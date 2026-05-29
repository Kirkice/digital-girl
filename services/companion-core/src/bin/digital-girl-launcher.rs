use chrono::Local;
use eframe::egui::{
    self, Align, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Layout,
    RichText, ScrollArea, Sense, Stroke, Vec2,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_LOG_LINES: usize = 500;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServiceId {
    CompanionCore,
    LiveTalking,
}

struct LogEvent {
    service: ServiceId,
    line: String,
}

enum UiAction {
    Start(ServiceId),
    Stop(ServiceId),
    Restart(ServiceId),
    ClearLogs(ServiceId),
}

#[derive(Clone, Copy)]
enum DiagnosticStatus {
    Pass,
    Warn,
    Fail,
}

struct DiagnosticItem {
    status: DiagnosticStatus,
    label: String,
    detail: String,
}

struct DiagnosticResult {
    items: Vec<DiagnosticItem>,
}

struct LlmConfigSummary {
    config_file_path: PathBuf,
    config_file_exists: bool,
    base_url: Option<String>,
    model: String,
    api_key_configured: bool,
}

#[derive(Clone, Copy)]
enum ConfigFileFormat {
    Toml,
    Env,
}

struct ConfigFileSource {
    path: PathBuf,
    format: ConfigFileFormat,
}

#[derive(Debug, Default, Deserialize)]
struct CompanionCoreTomlFile {
    llm: Option<CompanionCoreTomlLlm>,
}

#[derive(Debug, Default, Deserialize)]
struct CompanionCoreTomlLlm {
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
}

impl LlmConfigSummary {
    fn is_ready(&self) -> bool {
        self.base_url.is_some() && self.api_key_configured
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum AppPage {
    Dashboard,
    Chat,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
enum ChatRole {
    User,
    Assistant,
    System,
}

#[derive(Clone, Deserialize, Serialize)]
struct ChatMessage {
    role: ChatRole,
    content: String,
    meta: Option<String>,
    timestamp: String,
}

impl ChatMessage {
    fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
            meta: None,
            timestamp: current_chat_timestamp(),
        }
    }

    fn assistant(content: impl Into<String>, meta: Option<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
            meta,
            timestamp: current_chat_timestamp(),
        }
    }

    fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
            meta: None,
            timestamp: current_chat_timestamp(),
        }
    }
}

struct ChatCompletion {
    reply: String,
    source: String,
    detail: Option<String>,
}

struct ChatResult {
    outcome: Result<ChatCompletion, String>,
}

struct ChatStreamState {
    message_index: usize,
    revealed_chars: usize,
    total_chars: usize,
    last_tick: Instant,
}

#[derive(Deserialize, Serialize)]
struct PersistedChatState {
    session_id: String,
    messages: Vec<ChatMessage>,
}

struct ServerProcess {
    child: Child,
    pid: u32,
}

struct ServerPanel {
    service: ServiceId,
    name: &'static str,
    description: &'static str,
    port: u16,
    url: String,
    command_hint: String,
    process: Option<ServerProcess>,
    logs: VecDeque<String>,
    listening: bool,
    last_probe: Instant,
    last_exit: Option<String>,
}

impl ServerPanel {
    fn new(
        service: ServiceId,
        name: &'static str,
        description: &'static str,
        port: u16,
        url: impl Into<String>,
        command_hint: impl Into<String>,
    ) -> Self {
        Self {
            service,
            name,
            description,
            port,
            url: url.into(),
            command_hint: command_hint.into(),
            process: None,
            logs: VecDeque::new(),
            listening: false,
            last_probe: Instant::now() - Duration::from_secs(10),
            last_exit: None,
        }
    }

    fn is_running(&self) -> bool {
        self.process.is_some()
    }

    fn push_log(&mut self, line: impl Into<String>) {
        if self.logs.len() >= MAX_LOG_LINES {
            self.logs.pop_front();
        }
        self.logs.push_back(line.into());
    }

    fn status_label(&self) -> &'static str {
        if self.is_running() && self.listening {
            "Online"
        } else if self.is_running() {
            "Starting"
        } else if self.listening {
            "External"
        } else {
            "Stopped"
        }
    }

    fn status_color(&self) -> Color32 {
        if self.is_running() && self.listening {
            Color32::from_rgb(58, 201, 131)
        } else if self.is_running() {
            Color32::from_rgb(247, 196, 83)
        } else if self.listening {
            Color32::from_rgb(92, 162, 255)
        } else {
            Color32::from_rgb(128, 139, 151)
        }
    }
}

struct ServiceOverview {
    name: &'static str,
    port: u16,
    url: String,
    status_label: &'static str,
    status_color: Color32,
    running: bool,
    listening: bool,
}

impl From<&ServerPanel> for ServiceOverview {
    fn from(panel: &ServerPanel) -> Self {
        Self {
            name: panel.name,
            port: panel.port,
            url: panel.url.clone(),
            status_label: panel.status_label(),
            status_color: panel.status_color(),
            running: panel.is_running(),
            listening: panel.listening,
        }
    }
}

struct ControlPanelApp {
    project_root: PathBuf,
    companion_dir: PathBuf,
    livetalking_dir: PathBuf,
    python_exe: PathBuf,
    chat_state_path: PathBuf,
    current_page: AppPage,
    companion: ServerPanel,
    livetalking: ServerPanel,
    log_tx: Sender<LogEvent>,
    log_rx: Receiver<LogEvent>,
    diagnostic_tx: Sender<DiagnosticResult>,
    diagnostic_rx: Receiver<DiagnosticResult>,
    diagnostics: Vec<DiagnosticItem>,
    diagnostics_running: bool,
    diagnostics_message: String,
    llm_config: LlmConfigSummary,
    chat_session_id: String,
    chat_input: String,
    chat_messages: Vec<ChatMessage>,
    chat_running: bool,
    chat_stream: Option<ChatStreamState>,
    chat_tx: Sender<ChatResult>,
    chat_rx: Receiver<ChatResult>,
    last_error: Option<String>,
}

impl ControlPanelApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_fonts(&cc.egui_ctx);
        configure_style(&cc.egui_ctx);

        let auto_start_all = std::env::var("DIGITAL_GIRL_AUTOSTART_ALL")
            .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(false);

        let companion_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let project_root = companion_dir
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| companion_dir.clone());
        let livetalking_dir = project_root.join("backend").join("livetalking");
        let venv_python = project_root
            .join(".venv")
            .join("Scripts")
            .join("python.exe");
        let python_exe = if venv_python.exists() {
            venv_python
        } else {
            PathBuf::from("python")
        };

        let (log_tx, log_rx) = mpsc::channel();
        let (diagnostic_tx, diagnostic_rx) = mpsc::channel();
        let llm_config = load_llm_config_summary(&project_root);
        let chat_state_path = chat_state_path(&project_root);
        let persisted_chat = load_chat_state(&chat_state_path, &llm_config);
        let (chat_tx, chat_rx) = mpsc::channel();

        let mut app = Self {
            project_root,
            companion_dir,
            livetalking_dir,
            python_exe,
            chat_state_path,
            current_page: AppPage::Chat,
            companion: ServerPanel::new(
                ServiceId::CompanionCore,
                "companion-core",
                "Persona, memory, and chat routing sidecar",
                8787,
                "http://127.0.0.1:8787/health",
                "cargo run --bin companion-core",
            ),
            livetalking: ServerPanel::new(
                ServiceId::LiveTalking,
                "LiveTalking",
                "WebRTC digital human renderer",
                8010,
                "http://127.0.0.1:8010/index.html",
                "python app.py --transport webrtc --model wav2lip --avatar_id wav2lip256_avatar1 --listenport 8010",
            ),
            log_tx,
            log_rx,
            diagnostic_tx,
            diagnostic_rx,
            diagnostics: Vec::new(),
            diagnostics_running: false,
            diagnostics_message: "Diagnostics have not run yet.".to_string(),
            llm_config,
            chat_session_id: persisted_chat.session_id,
            chat_input: String::new(),
            chat_messages: persisted_chat.messages,
            chat_running: false,
            chat_stream: None,
            chat_tx,
            chat_rx,
            last_error: None,
        };

        app.run_diagnostics();
        if auto_start_all {
            app.start_service(ServiceId::CompanionCore);
            app.start_service(ServiceId::LiveTalking);
        }
        app
    }

    fn start_service(&mut self, service: ServiceId) {
        self.last_error = None;

        let result = match service {
            ServiceId::CompanionCore => self.start_companion_core(),
            ServiceId::LiveTalking => self.start_livetalking(),
        };

        if let Err(error) = result {
            self.last_error = Some(error);
        }
    }

    fn start_companion_core(&mut self) -> Result<(), String> {
        if self.companion.is_running() {
            return Ok(());
        }

        if !self.companion_dir.exists() {
            return Err(format!(
                "companion-core directory not found: {}",
                self.companion_dir.display()
            ));
        }

        if self.companion.listening {
            return Err(
                "port 8787 is already in use; stop the external companion-core process first"
                    .to_string(),
            );
        }

        let mut command = Command::new("cargo");
        command
            .current_dir(&self.companion_dir)
            .args(["run", "--bin", "companion-core"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_child_console(&mut command);

        let process = spawn_server_process(
            command,
            ServiceId::CompanionCore,
            self.log_tx.clone(),
            self.project_root.join("logs").join("companion-core.log"),
            &mut self.companion,
        )?;
        self.companion.process = Some(process);
        Ok(())
    }

    fn start_livetalking(&mut self) -> Result<(), String> {
        if self.livetalking.is_running() {
            return Ok(());
        }

        let app_py = self.livetalking_dir.join("app.py");
        if !app_py.exists() {
            return Err(format!(
                "LiveTalking app.py not found: {}",
                app_py.display()
            ));
        }

        if self.livetalking.listening {
            return Err(
                "port 8010 is already in use; stop the external LiveTalking process first"
                    .to_string(),
            );
        }

        let mut command = Command::new(&self.python_exe);
        command
            .current_dir(&self.livetalking_dir)
            .arg("app.py")
            .args([
                "--transport",
                "webrtc",
                "--model",
                "wav2lip",
                "--avatar_id",
                "wav2lip256_avatar1",
                "--listenport",
                "8010",
            ])
            .env("COMPANION_CORE_URL", "http://127.0.0.1:8787")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_child_console(&mut command);

        let process = spawn_server_process(
            command,
            ServiceId::LiveTalking,
            self.log_tx.clone(),
            self.project_root.join("logs").join("livetalking.log"),
            &mut self.livetalking,
        )?;
        self.livetalking.process = Some(process);
        Ok(())
    }

    fn stop_service(&mut self, service: ServiceId) {
        let panel = self.panel_mut(service);
        stop_panel_process(panel);
    }

    fn restart_service(&mut self, service: ServiceId) {
        self.stop_service(service);
        self.start_service(service);
    }

    fn clear_service_logs(&mut self, service: ServiceId) {
        self.panel_mut(service).logs.clear();
    }

    fn stop_all(&mut self) {
        stop_panel_process(&mut self.companion);
        stop_panel_process(&mut self.livetalking);
    }

    fn poll_processes(&mut self) {
        poll_panel_process(&mut self.companion);
        poll_panel_process(&mut self.livetalking);
    }

    fn drain_logs(&mut self) {
        while let Ok(event) = self.log_rx.try_recv() {
            self.panel_mut(event.service).push_log(event.line);
        }
    }

    fn drain_diagnostics(&mut self) {
        while let Ok(result) = self.diagnostic_rx.try_recv() {
            self.diagnostics = result.items;
            self.diagnostics_running = false;
            self.diagnostics_message = "Diagnostics completed.".to_string();
        }
    }

    fn push_chat_message(&mut self, message: ChatMessage) {
        self.chat_messages.push(message);
        if self.chat_messages.len() > 48 {
            let overflow = self.chat_messages.len() - 48;
            self.chat_messages.drain(0..overflow);
        }
        self.save_chat_state();
    }

    fn reset_chat_thread(&mut self) {
        self.chat_stream = None;
        self.chat_session_id = new_chat_session_id();
        self.chat_input.clear();
        self.chat_messages = default_chat_messages(&self.llm_config, &self.chat_session_id);
        self.save_chat_state();
    }

    fn drain_chat_results(&mut self) {
        while let Ok(result) = self.chat_rx.try_recv() {
            self.chat_running = false;
            match result.outcome {
                Ok(completion) => {
                    let meta = format_chat_completion_meta(&completion);
                    let reply = if completion.reply.trim().is_empty() {
                        "No reply text returned by companion-core.".to_string()
                    } else {
                        completion.reply
                    };
                    let message_index = self.chat_messages.len();
                    self.push_chat_message(ChatMessage::assistant(reply, Some(meta)));
                    self.start_chat_stream(message_index);
                }
                Err(error) => {
                    self.push_chat_message(ChatMessage::system(format!(
                        "Chat request failed. {error}"
                    )));
                }
            }
        }
    }

    fn start_chat_stream(&mut self, message_index: usize) {
        let total_chars = self
            .chat_messages
            .get(message_index)
            .map(|message| message.content.chars().count())
            .unwrap_or(0);

        if total_chars == 0 {
            self.chat_stream = None;
            return;
        }

        self.chat_stream = Some(ChatStreamState {
            message_index,
            revealed_chars: 0,
            total_chars,
            last_tick: Instant::now() - Duration::from_millis(24),
        });
    }

    fn advance_chat_stream(&mut self) {
        let Some(stream) = self.chat_stream.as_mut() else {
            return;
        };

        if stream.message_index >= self.chat_messages.len() {
            self.chat_stream = None;
            return;
        }

        let now = Instant::now();
        let elapsed = now.saturating_duration_since(stream.last_tick);
        if elapsed < Duration::from_millis(18) {
            return;
        }

        let steps = ((elapsed.as_millis() / 18).max(1)) as usize;
        let chars_per_step = match stream.total_chars {
            0..=72 => 2,
            73..=180 => 4,
            _ => 7,
        };

        stream.revealed_chars = (stream.revealed_chars + steps * chars_per_step).min(stream.total_chars);
        stream.last_tick = now;

        if stream.revealed_chars >= stream.total_chars {
            self.chat_stream = None;
        }
    }

    fn chat_busy(&self) -> bool {
        self.chat_running || self.chat_stream.is_some()
    }

    fn save_chat_state(&self) {
        let state = PersistedChatState {
            session_id: self.chat_session_id.clone(),
            messages: self.chat_messages.clone(),
        };
        write_chat_state(&self.chat_state_path, &state);
    }

    fn run_diagnostics(&mut self) {
        if self.diagnostics_running {
            return;
        }

        self.diagnostics_running = true;
        self.diagnostics_message = "Diagnostics are running...".to_string();
        self.diagnostics.clear();

        let project_root = self.project_root.clone();
        let companion_dir = self.companion_dir.clone();
        let livetalking_dir = self.livetalking_dir.clone();
        let python_exe = self.python_exe.clone();
        let diagnostic_tx = self.diagnostic_tx.clone();

        thread::spawn(move || {
            let items =
                collect_diagnostics(project_root, companion_dir, livetalking_dir, python_exe);
            let _ = diagnostic_tx.send(DiagnosticResult { items });
        });
    }

    fn reload_llm_config(&mut self) {
        self.llm_config = load_llm_config_summary(&self.project_root);
    }

    fn send_chat_message(&mut self) {
        if self.chat_busy() {
            return;
        }

        let prompt = self.chat_input.trim().to_string();
        if prompt.is_empty() {
            return;
        }

        if !self.companion.listening {
            self.push_chat_message(ChatMessage::system(
                "Start companion-core first. This workspace sends messages through http://127.0.0.1:8787/chat.",
            ));
            return;
        }

        self.push_chat_message(ChatMessage::user(prompt.clone()));
        self.chat_input.clear();
        self.chat_running = true;
        self.chat_stream = None;
        let tx = self.chat_tx.clone();
        let session_id = self.chat_session_id.clone();

        thread::spawn(move || {
            let _ = tx.send(ChatResult {
                outcome: post_companion_chat_message(&session_id, &prompt),
            });
        });
    }

    fn refresh_ports(&mut self) {
        refresh_panel_port(&mut self.companion);
        refresh_panel_port(&mut self.livetalking);
    }

    fn panel_mut(&mut self, service: ServiceId) -> &mut ServerPanel {
        match service {
            ServiceId::CompanionCore => &mut self.companion,
            ServiceId::LiveTalking => &mut self.livetalking,
        }
    }
}

impl Drop for ControlPanelApp {
    fn drop(&mut self) {
        self.save_chat_state();
        self.stop_all();
    }
}

impl eframe::App for ControlPanelApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.drain_logs();
        self.drain_diagnostics();
        self.drain_chat_results();
        self.advance_chat_stream();
        self.poll_processes();
        self.refresh_ports();

        let animation_time = ctx.input(|input| input.time as f32);
        let chat_busy = self.chat_busy();

        let companion_overview = ServiceOverview::from(&self.companion);
        let livetalking_overview = ServiceOverview::from(&self.livetalking);

        let mut start_all_clicked = false;
        let mut stop_all_clicked = false;
        let mut open_digital_human_clicked = false;
        let mut run_diagnostics_clicked = false;
        let mut reload_llm_clicked = false;
        let mut send_chat_clicked = false;
        let mut new_thread_clicked = false;
        let mut open_chat_clicked = false;
        let mut next_page = None;
        let mut actions = Vec::new();

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(app_bg()))
            .show(ctx, |ui| {
                draw_background(ui);

                ScrollArea::vertical()
                    .id_source("control-center-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(14.0, 14.0);
                        ui.add_space(14.0);

                        draw_hero_header(
                            ui,
                            &companion_overview,
                            &livetalking_overview,
                            &self.llm_config,
                            &mut start_all_clicked,
                            &mut stop_all_clicked,
                            &mut open_digital_human_clicked,
                        );

                        if let Some(error) = &self.last_error {
                            draw_error_banner(ui, error);
                        }

                        if let Some(page) = draw_page_tabs(ui, self.current_page) {
                            next_page = Some(page);
                        }

                        match self.current_page {
                            AppPage::Dashboard => {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(292.0);
                                        draw_control_rail(
                                            ui,
                                            &companion_overview,
                                            &livetalking_overview,
                                            &self.llm_config,
                                            &self.diagnostics,
                                            self.diagnostics_running,
                                        );
                                    });

                                    ui.add_space(12.0);

                                    ui.vertical(|ui| {
                                        ui.set_width(ui.available_width());

                                        ui.columns(2, |columns| {
                                            if let Some(action) =
                                                draw_server_card(&mut columns[0], &mut self.companion)
                                            {
                                                actions.push(action);
                                            }
                                            if let Some(action) =
                                                draw_server_card(&mut columns[1], &mut self.livetalking)
                                            {
                                                actions.push(action);
                                            }
                                        });

                                        ui.add_space(2.0);
                                        if draw_chat_preview_panel(
                                            ui,
                                            &self.llm_config,
                                            &self.chat_messages,
                                            self.chat_running,
                                        ) {
                                            open_chat_clicked = true;
                                        }

                                        ui.add_space(2.0);
                                        ui.columns(2, |columns| {
                                            if draw_diagnostics_panel(
                                                &mut columns[0],
                                                &self.diagnostics,
                                                self.diagnostics_running,
                                                &self.diagnostics_message,
                                            ) {
                                                run_diagnostics_clicked = true;
                                            }

                                            draw_runtime_paths_panel(
                                                &mut columns[1],
                                                &self.project_root,
                                                &self.livetalking_dir,
                                                &self.python_exe,
                                            );
                                        });
                                    });
                                });
                            }
                            AppPage::Chat => {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_width(292.0);
                                        draw_chat_side_panel(
                                            ui,
                                            &companion_overview,
                                            &livetalking_overview,
                                            &self.llm_config,
                                            &self.chat_session_id,
                                        );
                                    });

                                    ui.add_space(12.0);

                                    ui.vertical(|ui| {
                                        ui.set_width(ui.available_width());
                                        let (send_clicked, reload_clicked, thread_clicked) =
                                            draw_chat_workspace(
                                                ui,
                                                &self.llm_config,
                                                &self.chat_session_id,
                                                &self.chat_messages,
                                                self.chat_stream.as_ref(),
                                                &mut self.chat_input,
                                                chat_busy,
                                                animation_time,
                                            );
                                        if send_clicked {
                                            send_chat_clicked = true;
                                        }
                                        if reload_clicked {
                                            reload_llm_clicked = true;
                                        }
                                        if thread_clicked {
                                            new_thread_clicked = true;
                                        }
                                    });
                                });
                            }
                        }

                        ui.add_space(16.0);
                    });
            });

        for action in actions {
            match action {
                UiAction::Start(service) => self.start_service(service),
                UiAction::Stop(service) => self.stop_service(service),
                UiAction::Restart(service) => self.restart_service(service),
                UiAction::ClearLogs(service) => self.clear_service_logs(service),
            }
        }

        if start_all_clicked {
            self.start_service(ServiceId::CompanionCore);
            self.start_service(ServiceId::LiveTalking);
        }
        if stop_all_clicked {
            self.stop_all();
        }
        if open_digital_human_clicked {
            open_url(&self.livetalking.url);
        }
        if run_diagnostics_clicked {
            self.run_diagnostics();
        }
        if reload_llm_clicked {
            self.reload_llm_config();
        }
        if let Some(page) = next_page {
            self.current_page = page;
        }
        if open_chat_clicked {
            self.current_page = AppPage::Chat;
        }
        if new_thread_clicked {
            self.reset_chat_thread();
        }
        if send_chat_clicked {
            self.send_chat_message();
        }

        let repaint_interval = if matches!(self.current_page, AppPage::Chat)
            && (chat_busy || !self.chat_input.trim().is_empty())
        {
            Duration::from_millis(33)
        } else {
            Duration::from_millis(500)
        };
        ctx.request_repaint_after(repaint_interval);
    }
}

fn app_bg() -> Color32 {
    Color32::from_rgb(10, 12, 16)
}

fn panel_bg() -> Color32 {
    Color32::from_rgb(18, 21, 26)
}

fn panel_bg_alt() -> Color32 {
    Color32::from_rgb(24, 27, 33)
}

fn panel_stroke() -> Stroke {
    Stroke::new(1.0, Color32::from_rgb(48, 53, 64))
}

fn text_primary() -> Color32 {
    Color32::from_rgb(241, 243, 247)
}

fn text_secondary() -> Color32 {
    Color32::from_rgb(137, 143, 154)
}

fn draw_background(ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, app_bg());
}

fn draw_hero_header(
    ui: &mut egui::Ui,
    companion: &ServiceOverview,
    livetalking: &ServiceOverview,
    llm_config: &LlmConfigSummary,
    start_all_clicked: &mut bool,
    stop_all_clicked: &mut bool,
    open_digital_human_clicked: &mut bool,
) {
    let (stack_label, stack_detail, stack_color) = stack_status(companion, livetalking);

    egui::Frame::none()
        .fill(panel_bg())
        .stroke(panel_stroke())
        .rounding(14.0)
        .inner_margin(egui::Margin::symmetric(20.0, 18.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("DIGITAL GIRL")
                            .font(FontId::proportional(12.0))
                            .color(text_secondary()),
                    );
                    ui.label(
                        RichText::new("Control Center")
                            .font(FontId::proportional(28.0))
                            .strong()
                            .color(text_primary()),
                    );
                    ui.label(
                        RichText::new("Local runtime, diagnostics, and LLM routing")
                            .color(text_secondary()),
                    );
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_sized([126.0, 34.0], egui::Button::new("Open Client"))
                        .clicked()
                    {
                        *open_digital_human_clicked = true;
                    }
                    if ui
                        .add_sized([82.0, 34.0], egui::Button::new("Stop All"))
                        .clicked()
                    {
                        *stop_all_clicked = true;
                    }
                    if ui
                        .add_sized([82.0, 34.0], egui::Button::new("Start All"))
                        .clicked()
                    {
                        *start_all_clicked = true;
                    }
                });
            });

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                draw_summary_tile(ui, "Stack", stack_label, stack_detail, stack_color);
                draw_summary_tile(
                    ui,
                    "companion-core",
                    companion.status_label,
                    &format!("{} · {}", companion.port, companion.url),
                    companion.status_color,
                );
                draw_summary_tile(
                    ui,
                    "LiveTalking",
                    livetalking.status_label,
                    &format!("{} · {}", livetalking.port, livetalking.url),
                    livetalking.status_color,
                );
                draw_summary_tile(
                    ui,
                    "LLM",
                    if llm_config.is_ready() {
                        "Configured"
                    } else {
                        "Local Fallback"
                    },
                    &format!("model: {}", llm_config.model),
                    if llm_config.is_ready() {
                        Color32::from_rgb(58, 201, 131)
                    } else {
                        Color32::from_rgb(247, 196, 83)
                    },
                );
            });
        });
}

fn draw_summary_tile(ui: &mut egui::Ui, label: &str, value: &str, detail: &str, color: Color32) {
    egui::Frame::none()
        .fill(panel_bg_alt())
        .stroke(panel_stroke())
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(12.0, 12.0))
        .show(ui, |ui| {
            ui.set_min_width(172.0);
            ui.horizontal(|ui| {
                status_dot(ui, color);
                ui.label(RichText::new(label).color(text_secondary()));
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(value)
                    .font(FontId::proportional(17.0))
                    .strong()
                    .color(text_primary()),
            );
            ui.label(RichText::new(detail).color(text_secondary()));
        });
}

fn draw_error_banner(ui: &mut egui::Ui, error: &str) {
    egui::Frame::none()
        .fill(Color32::from_rgb(48, 24, 28))
        .stroke(Stroke::new(1.0, Color32::from_rgb(82, 47, 54)))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                status_pill(ui, "ERROR", Color32::from_rgb(218, 103, 119));
                ui.label(RichText::new(error).color(Color32::from_rgb(238, 209, 215)));
            });
        });
}

fn draw_page_tabs(ui: &mut egui::Ui, current_page: AppPage) -> Option<AppPage> {
    let mut next_page = None;

    egui::Frame::none()
        .fill(panel_bg())
        .stroke(panel_stroke())
        .rounding(14.0)
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Page Mode").color(text_secondary()));
                ui.add_space(8.0);

                for (page, label) in [
                    (AppPage::Dashboard, "Dashboard"),
                    (AppPage::Chat, "Chat Studio"),
                ] {
                    let selected = current_page == page;
                    let fill = if selected {
                        Color32::from_rgb(34, 40, 52)
                    } else {
                        Color32::from_rgb(19, 23, 30)
                    };
                    let stroke = if selected {
                        Stroke::new(1.0, Color32::from_rgb(88, 132, 255))
                    } else {
                        panel_stroke()
                    };

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(label)
                                    .strong()
                                    .color(if selected {
                                        text_primary()
                                    } else {
                                        text_secondary()
                                    }),
                            )
                            .fill(fill)
                            .stroke(stroke)
                            .min_size(Vec2::new(126.0, 34.0)),
                        )
                        .clicked()
                    {
                        next_page = Some(page);
                    }
                }
            });
        });

    next_page
}

fn draw_chat_preview_panel(
    ui: &mut egui::Ui,
    config: &LlmConfigSummary,
    messages: &[ChatMessage],
    chat_running: bool,
) -> bool {
    let mut open_clicked = false;
    let last_message = messages.last();

    egui::Frame::none()
        .fill(panel_bg())
        .stroke(panel_stroke())
        .rounding(16.0)
        .inner_margin(egui::Margin::symmetric(18.0, 16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Chat Studio")
                            .font(FontId::proportional(20.0))
                            .strong()
                            .color(text_primary()),
                    );
                    ui.label(
                        RichText::new("Standalone conversation mode with richer bubbles and thread context")
                            .color(text_secondary()),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new("Open Full Chat")
                                .fill(Color32::from_rgb(84, 125, 255))
                                .stroke(Stroke::new(1.0, Color32::from_rgb(108, 146, 255))),
                        )
                        .clicked()
                    {
                        open_clicked = true;
                    }
                    status_pill(
                        ui,
                        if config.is_ready() { "LIVE" } else { "LOCAL" },
                        if config.is_ready() {
                            Color32::from_rgb(58, 201, 131)
                        } else {
                            Color32::from_rgb(247, 196, 83)
                        },
                    );
                });
            });

            ui.add_space(12.0);
            egui::Frame::none()
                .fill(Color32::from_rgb(16, 19, 24))
                .stroke(Stroke::new(1.0, Color32::from_rgb(44, 50, 62)))
                .rounding(14.0)
                .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                .show(ui, |ui| {
                    if let Some(message) = last_message {
                        ui.label(
                            RichText::new(match message.role {
                                ChatRole::User => "Latest user turn",
                                ChatRole::Assistant => "Latest assistant turn",
                                ChatRole::System => "Latest system note",
                            })
                            .size(11.0)
                            .strong()
                            .color(Color32::from_rgb(106, 171, 255)),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(truncate_ui_detail(&message.content, 180))
                                .size(15.0)
                                .color(text_primary()),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!(
                                "{} • {}",
                                message.timestamp,
                                if chat_running {
                                    "assistant is responding"
                                } else {
                                    "chat history is ready"
                                }
                            ))
                            .color(text_secondary()),
                        );
                    } else {
                        ui.label(RichText::new("No messages yet.").color(text_secondary()));
                    }
                });
        });

    open_clicked
}

fn draw_chat_side_panel(
    ui: &mut egui::Ui,
    companion: &ServiceOverview,
    livetalking: &ServiceOverview,
    llm_config: &LlmConfigSummary,
    session_id: &str,
) {
    egui::Frame::none()
        .fill(panel_bg())
        .stroke(panel_stroke())
        .rounding(16.0)
        .inner_margin(egui::Margin::symmetric(16.0, 16.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Chat Session")
                    .font(FontId::proportional(20.0))
                    .strong()
                    .color(text_primary()),
            );
            ui.label(
                RichText::new("Dedicated conversation mode with runtime context kept nearby")
                    .color(text_secondary()),
            );

            ui.add_space(14.0);
            draw_chat_status_tile(
                ui,
                "Route",
                if llm_config.is_ready() {
                    "Cloud Ready"
                } else {
                    "Local Fallback"
                },
                &format!("model: {}", llm_config.model),
                if llm_config.is_ready() {
                    Color32::from_rgb(58, 201, 131)
                } else {
                    Color32::from_rgb(247, 196, 83)
                },
            );
            ui.add_space(8.0);
            draw_chat_status_tile(
                ui,
                "Thread",
                &compact_session_id(session_id),
                "new threads generate a fresh local session id",
                Color32::from_rgb(236, 91, 110),
            );
            ui.add_space(8.0);
            draw_chat_status_tile(
                ui,
                "companion-core",
                companion.status_label,
                &format!("port {}", companion.port),
                companion.status_color,
            );
            ui.add_space(8.0);
            draw_chat_status_tile(
                ui,
                "LiveTalking",
                livetalking.status_label,
                &format!("port {}", livetalking.port),
                livetalking.status_color,
            );

            ui.add_space(12.0);
            egui::Frame::none()
                .fill(panel_bg_alt())
                .stroke(panel_stroke())
                .rounding(12.0)
                .inner_margin(egui::Margin::symmetric(12.0, 12.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("Interaction Notes").strong().color(text_primary()));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Ctrl+Enter sends the current draft.").color(text_secondary()));
                    ui.label(RichText::new("New Thread keeps the UI but rotates the session id.").color(text_secondary()));
                    ui.label(RichText::new("Bubble headers now show role, avatar, and message time.").color(text_secondary()));
                });
        });
}

fn draw_chat_status_tile(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    detail: &str,
    accent: Color32,
) {
    egui::Frame::none()
        .fill(panel_bg_alt())
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 90),
        ))
        .rounding(12.0)
        .inner_margin(egui::Margin::symmetric(12.0, 12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                status_dot(ui, accent);
                ui.label(RichText::new(label).color(text_secondary()));
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(value)
                    .font(FontId::proportional(17.0))
                    .strong()
                    .color(text_primary()),
            );
            ui.label(RichText::new(detail).color(text_secondary()));
        });
}

fn draw_control_rail(
    ui: &mut egui::Ui,
    companion: &ServiceOverview,
    livetalking: &ServiceOverview,
    llm_config: &LlmConfigSummary,
    diagnostics: &[DiagnosticItem],
    diagnostics_running: bool,
) {
    let (pass_count, warn_count, fail_count) = diagnostic_counts(diagnostics);

    egui::Frame::none()
        .fill(panel_bg())
        .stroke(panel_stroke())
        .rounding(14.0)
        .inner_margin(egui::Margin::symmetric(16.0, 16.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new("Overview")
                    .font(FontId::proportional(19.0))
                    .strong()
                    .color(text_primary()),
            );
            ui.label(RichText::new("Live status for the local stack").color(text_secondary()));

            ui.add_space(14.0);
            draw_side_service_row(ui, companion);
            draw_side_service_row(ui, livetalking);

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);

            draw_metric_tile(
                ui,
                "LLM Mode",
                if llm_config.is_ready() {
                    "Cloud Ready"
                } else {
                    "Local Fallback"
                },
                &format!("model: {}", llm_config.model),
                if llm_config.is_ready() {
                    Color32::from_rgb(58, 201, 131)
                } else {
                    Color32::from_rgb(247, 196, 83)
                },
            );

            ui.add_space(8.0);
            draw_metric_tile(
                ui,
                "Diagnostics",
                if diagnostics_running {
                    "Running"
                } else if diagnostics.is_empty() {
                    "Not Run"
                } else if fail_count > 0 {
                    "Needs Attention"
                } else if warn_count > 0 {
                    "Warnings"
                } else {
                    "Healthy"
                },
                &format!("pass {pass_count} / warn {warn_count} / fail {fail_count}"),
                if fail_count > 0 {
                    Color32::from_rgb(236, 91, 110)
                } else if warn_count > 0 || diagnostics_running {
                    Color32::from_rgb(247, 196, 83)
                } else {
                    Color32::from_rgb(58, 201, 131)
                },
            );

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);
            ui.label(RichText::new("Endpoints").strong().color(text_primary()));
            ui.add_space(6.0);
            ui.monospace(&companion.url);
            ui.monospace(&livetalking.url);
        });
}

fn draw_side_service_row(ui: &mut egui::Ui, panel: &ServiceOverview) {
    ui.horizontal(|ui| {
        status_dot(ui, panel.status_color);
        ui.vertical(|ui| {
            ui.label(RichText::new(panel.name).strong().color(text_primary()));
            let ownership = if panel.running {
                "managed"
            } else if panel.listening {
                "external"
            } else {
                "idle"
            };
            ui.label(
                RichText::new(format!(
                    "{} · port {} · {}",
                    panel.status_label, panel.port, ownership
                ))
                .color(text_secondary()),
            );
        });
    });
    ui.add_space(8.0);
}

fn draw_metric_tile(ui: &mut egui::Ui, label: &str, value: &str, detail: &str, color: Color32) {
    egui::Frame::none()
        .fill(panel_bg_alt())
        .stroke(panel_stroke())
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(12.0, 12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                status_dot(ui, color);
                ui.label(RichText::new(label).color(text_secondary()));
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(value)
                    .font(FontId::proportional(17.0))
                    .strong()
                    .color(text_primary()),
            );
            ui.label(RichText::new(detail).color(text_secondary()));
        });
}

fn draw_runtime_paths_panel(
    ui: &mut egui::Ui,
    project_root: &Path,
    livetalking_dir: &Path,
    python_exe: &Path,
) {
    egui::Frame::none()
        .fill(panel_bg())
        .stroke(panel_stroke())
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Runtime Paths")
                        .font(FontId::proportional(17.0))
                        .strong()
                        .color(text_primary()),
                );
                ui.label(
                    RichText::new("resolved from the current workspace").color(text_secondary()),
                );
            });
            ui.add_space(8.0);
            ui.monospace(format!("Project       {}", project_root.display()));
            ui.monospace(format!("LiveTalking   {}", livetalking_dir.display()));
            ui.monospace(format!("Python        {}", python_exe.display()));
        });
}

fn stack_status(
    companion: &ServiceOverview,
    livetalking: &ServiceOverview,
) -> (&'static str, &'static str, Color32) {
    if companion.running && companion.listening && livetalking.running && livetalking.listening {
        (
            "Running",
            "both managed services are available",
            Color32::from_rgb(58, 201, 131),
        )
    } else if companion.running || livetalking.running {
        (
            "Booting",
            "one or more services are starting",
            Color32::from_rgb(247, 196, 83),
        )
    } else if companion.listening || livetalking.listening {
        (
            "EXTERNAL",
            "ports are owned by external processes",
            Color32::from_rgb(92, 162, 255),
        )
    } else {
        (
            "Idle",
            "ready to launch the stack",
            Color32::from_rgb(128, 139, 151),
        )
    }
}

fn diagnostic_counts(diagnostics: &[DiagnosticItem]) -> (usize, usize, usize) {
    let mut pass_count = 0;
    let mut warn_count = 0;
    let mut fail_count = 0;

    for item in diagnostics {
        match item.status {
            DiagnosticStatus::Pass => pass_count += 1,
            DiagnosticStatus::Warn => warn_count += 1,
            DiagnosticStatus::Fail => fail_count += 1,
        }
    }

    (pass_count, warn_count, fail_count)
}

fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
    ui.painter().circle_stroke(
        rect.center(),
        4.5,
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 96),
        ),
    );
}

fn draw_server_card(ui: &mut egui::Ui, panel: &mut ServerPanel) -> Option<UiAction> {
    let mut action = None;

    ui.push_id(panel.name, |ui| {
        egui::Frame::none()
            .fill(panel_bg())
            .stroke(panel_stroke())
            .rounding(12.0)
            .inner_margin(egui::Margin::symmetric(16.0, 16.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(panel.name)
                                .font(FontId::proportional(21.0))
                                .strong()
                                .color(text_primary()),
                        );
                        ui.label(RichText::new(panel.description).color(text_secondary()));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        status_pill(ui, panel.status_label(), panel.status_color());
                    });
                });

                ui.add_space(12.0);
                egui::Frame::none()
                    .fill(panel_bg_alt())
                    .stroke(panel_stroke())
                    .rounding(10.0)
                    .inner_margin(egui::Margin::symmetric(12.0, 12.0))
                    .show(ui, |ui| {
                        ui.monospace(format!("URL      {}", panel.url));
                        ui.monospace(format!("Command  {}", panel.command_hint));
                        if let Some(exit) = &panel.last_exit {
                            ui.monospace(format!("Last     {exit}"));
                        }
                    });

                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    let start_enabled = !panel.is_running() && !panel.listening;
                    if ui
                        .add_enabled(
                            start_enabled,
                            egui::Button::new("Start").min_size(Vec2::new(80.0, 30.0)),
                        )
                        .clicked()
                    {
                        action = Some(UiAction::Start(panel.service));
                    }
                    if ui
                        .add_enabled(
                            panel.is_running(),
                            egui::Button::new("Stop").min_size(Vec2::new(80.0, 30.0)),
                        )
                        .clicked()
                    {
                        action = Some(UiAction::Stop(panel.service));
                    }
                    if ui
                        .add_enabled(
                            panel.is_running(),
                            egui::Button::new("Restart").min_size(Vec2::new(80.0, 30.0)),
                        )
                        .clicked()
                    {
                        action = Some(UiAction::Restart(panel.service));
                    }
                    if ui.button("Clear Logs").clicked() {
                        action = Some(UiAction::ClearLogs(panel.service));
                    }
                    if ui.button("Open URL").clicked() {
                        open_url(&panel.url);
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(RichText::new("Logs").strong().color(text_primary()));
                ScrollArea::vertical()
                    .id_source((panel.name, "logs-scroll"))
                    .max_height(285.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if panel.logs.is_empty() {
                            ui.label(
                                RichText::new(
                                    "No logs yet. Start the service to stream output here.",
                                )
                                .color(text_secondary()),
                            );
                        } else {
                            for line in &panel.logs {
                                ui.monospace(line);
                            }
                        }
                    });
            });
    });

    action
}

fn draw_diagnostics_panel(
    ui: &mut egui::Ui,
    diagnostics: &[DiagnosticItem],
    running: bool,
    message: &str,
) -> bool {
    let mut run_clicked = false;

    egui::Frame::none()
        .fill(panel_bg())
        .stroke(panel_stroke())
        .rounding(12.0)
        .inner_margin(egui::Margin::symmetric(16.0, 16.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Diagnostics")
                            .font(FontId::proportional(20.0))
                            .strong()
                            .color(text_primary()),
                    );
                    ui.label(RichText::new(message).color(text_secondary()));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(!running, egui::Button::new("Run Checks"))
                        .clicked()
                    {
                        run_clicked = true;
                    }
                });
            });

            ui.add_space(10.0);
            if running {
                ui.label(
                    RichText::new("Checking Python, assets, and ports...")
                        .color(Color32::from_rgb(247, 196, 83)),
                );
            } else if diagnostics.is_empty() {
                ui.label(RichText::new("No diagnostics yet.").color(text_secondary()));
            } else {
                ScrollArea::vertical()
                    .id_source("diagnostics-scroll")
                    .max_height(280.0)
                    .show(ui, |ui| {
                    for item in diagnostics {
                        draw_diagnostic_item(ui, item);
                        ui.add_space(4.0);
                    }
                });
            }
        });

    run_clicked
}

fn draw_chat_workspace(
    ui: &mut egui::Ui,
    config: &LlmConfigSummary,
    session_id: &str,
    messages: &[ChatMessage],
    chat_stream: Option<&ChatStreamState>,
    chat_input: &mut String,
    chat_busy: bool,
    animation_time: f32,
) -> (bool, bool, bool) {
    let mut send_clicked = false;
    let mut reload_clicked = false;
    let mut new_thread_clicked = false;
    let input_has_content = !chat_input.trim().is_empty();
    let (status_text, status_color) = if config.is_ready() {
        ("LIVE ROUTE", Color32::from_rgb(58, 201, 131))
    } else {
        ("LOCAL FALLBACK", Color32::from_rgb(247, 196, 83))
    };
    let composer_accent = composer_accent_color(animation_time, chat_busy, input_has_content);
    let composer_fill = blend_color(
        Color32::from_rgb(19, 23, 30),
        Color32::from_rgb(27, 33, 43),
        if chat_busy {
            0.35
        } else if input_has_content {
            0.18
        } else {
            0.0
        },
    );

    egui::Frame::none()
        .fill(panel_bg())
        .stroke(panel_stroke())
        .rounding(18.0)
        .inner_margin(egui::Margin::symmetric(18.0, 18.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Conversation Studio")
                            .font(FontId::proportional(22.0))
                            .strong()
                            .color(text_primary()),
                    );
                    ui.label(
                        RichText::new(
                            "A dedicated bubble chat workspace powered by companion-core /chat",
                        )
                            .color(text_secondary()),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    status_pill(ui, status_text, status_color);
                    if chat_busy {
                        ui.label(
                            RichText::new("assistant is responding")
                                .color(Color32::from_rgb(247, 196, 83)),
                        );
                    }
                });
            });

            ui.add_space(10.0);
            egui::Frame::none()
                .fill(panel_bg_alt())
                .stroke(Stroke::new(1.0, Color32::from_rgb(54, 61, 76)))
                .rounding(14.0)
                .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        draw_chat_stat_chip(ui, "Model", &config.model, Color32::from_rgb(88, 132, 255));
                        draw_chat_stat_chip(
                            ui,
                            "Endpoint",
                            config
                                .base_url
                                .as_deref()
                                .map(|value| truncate_ui_detail(value, 34))
                                .as_deref()
                                .unwrap_or("not configured"),
                            Color32::from_rgb(58, 201, 131),
                        );
                        draw_chat_stat_chip(
                            ui,
                            "Config",
                            &format!(
                                "{} ({})",
                                config
                                    .config_file_path
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("companion-core.toml"),
                                if config.config_file_exists { "found" } else { "missing" }
                            ),
                            Color32::from_rgb(247, 196, 83),
                        );
                        draw_chat_stat_chip(
                            ui,
                            "Session",
                            &compact_session_id(session_id),
                            Color32::from_rgb(236, 91, 110),
                        );
                    });
                });

            ui.add_space(10.0);
            egui::Frame::none()
                .fill(Color32::from_rgb(14, 17, 22))
                .stroke(Stroke::new(1.0, Color32::from_rgb(42, 47, 58)))
                .rounding(18.0)
                .inner_margin(egui::Margin::symmetric(16.0, 16.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Transcript")
                                .font(FontId::proportional(17.0))
                                .strong()
                                .color(text_primary()),
                        );
                        ui.label(
                            RichText::new("Bubble chat routed through the active local stack")
                                .color(text_secondary()),
                        );
                    });
                    ui.add_space(10.0);
                    ScrollArea::vertical()
                        .id_source("chat-transcript-scroll")
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .max_height(520.0)
                        .show(ui, |ui| {
                            if messages.is_empty() {
                                ui.label(
                                    RichText::new("No messages yet. Start the conversation below.")
                                        .color(text_secondary()),
                                );
                            } else {
                                for (message_index, message) in messages.iter().enumerate() {
                                    draw_chat_message(ui, message, message_index, chat_stream);
                                    ui.add_space(10.0);
                                }
                            }
                            if chat_busy && chat_stream.is_none() {
                                draw_chat_message(
                                    ui,
                                    &ChatMessage::assistant(
                                        "Thinking...",
                                        Some("waiting for companion-core".to_string()),
                                    ),
                                    usize::MAX,
                                    None,
                                );
                            }
                        });
                });

            ui.add_space(10.0);
            egui::Frame::none()
                .fill(composer_fill)
                .stroke(Stroke::new(
                    if chat_busy || input_has_content { 1.5 } else { 1.0 },
                    composer_accent,
                ))
                .rounding(16.0)
                .inner_margin(egui::Margin::symmetric(14.0, 14.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Composer")
                                .strong()
                                .color(text_primary()),
                        );
                        ui.label(
                            RichText::new(if chat_busy {
                                "response in flight"
                            } else if input_has_content {
                                "draft active"
                            } else {
                                "ready"
                            })
                            .color(composer_accent),
                        );
                    });
                    ui.add_space(6.0);
                    draw_chat_activity_bar(
                        ui,
                        animation_time,
                        composer_accent,
                        chat_busy,
                        input_has_content,
                    );
                    ui.add_space(10.0);
                    let response = ui.add(
                        egui::TextEdit::multiline(chat_input)
                            .id_source("chat-composer-input")
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .hint_text("Type a message for Digital Girl..."),
                    );
                    let send_shortcut = response.has_focus()
                        && ui.input(|input| {
                            input.modifiers.ctrl && input.key_pressed(egui::Key::Enter)
                        });

                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(if chat_busy {
                                "Waiting for the current reply..."
                            } else {
                                "Enter inserts a new line. Ctrl+Enter sends."
                            })
                            .color(if chat_busy {
                                Color32::from_rgb(247, 196, 83)
                            } else {
                                text_secondary()
                            }),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    !chat_busy && !chat_input.trim().is_empty(),
                                    egui::Button::new(
                                        RichText::new("Send")
                                            .strong()
                                            .color(Color32::from_rgb(248, 250, 255)),
                                    )
                                    .fill(Color32::from_rgb(84, 125, 255))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(108, 146, 255))),
                                )
                                .clicked()
                            {
                                send_clicked = true;
                            }

                            if ui
                                .add_enabled(!chat_busy, egui::Button::new("New Thread"))
                                .clicked()
                            {
                                new_thread_clicked = true;
                            }

                            if ui.button("Reload Config").clicked() {
                                reload_clicked = true;
                            }
                        });
                    });

                    if send_shortcut && !chat_busy && !chat_input.trim().is_empty() {
                        send_clicked = true;
                    }
                });
        });

    (send_clicked, reload_clicked, new_thread_clicked)
}

fn draw_chat_stat_chip(ui: &mut egui::Ui, label: &str, value: &str, accent: Color32) {
    egui::Frame::none()
        .fill(Color32::from_rgb(20, 24, 31))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96),
        ))
        .rounding(12.0)
        .inner_margin(egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(10.0).color(text_secondary()));
            ui.label(
                RichText::new(value)
                    .size(13.0)
                    .strong()
                    .color(text_primary()),
            );
        });
}

fn draw_chat_message(
    ui: &mut egui::Ui,
    message: &ChatMessage,
    message_index: usize,
    chat_stream: Option<&ChatStreamState>,
) {
    let right_aligned = matches!(message.role, ChatRole::User);
    let layout = if right_aligned {
        Layout::right_to_left(Align::TOP)
    } else {
        Layout::left_to_right(Align::TOP)
    };
    let visible_content = visible_chat_content(message, message_index, chat_stream);
    let visible_meta = visible_chat_meta(message, message_index, chat_stream);

    ui.with_layout(layout, |ui| {
        ui.scope(|ui| {
            ui.set_max_width(ui.available_width().min(560.0));

            let (label, avatar_label, avatar_fill, bubble_fill, bubble_stroke, label_color, text_color, meta_color) =
                match message.role {
                    ChatRole::User => (
                        "YOU",
                        "YU",
                        Color32::from_rgb(82, 118, 255),
                        Color32::from_rgb(82, 118, 255),
                        Stroke::new(1.0, Color32::from_rgb(112, 147, 255)),
                        Color32::from_rgb(170, 194, 255),
                        Color32::from_rgb(248, 250, 255),
                        Color32::from_rgba_unmultiplied(248, 250, 255, 190),
                    ),
                    ChatRole::Assistant => (
                        "DIGITAL GIRL",
                        "DG",
                        Color32::from_rgb(47, 81, 164),
                        Color32::from_rgb(30, 35, 44),
                        Stroke::new(1.0, Color32::from_rgb(58, 65, 80)),
                        Color32::from_rgb(106, 171, 255),
                        text_primary(),
                        text_secondary(),
                    ),
                    ChatRole::System => (
                        "SYSTEM",
                        "SY",
                        Color32::from_rgb(116, 60, 72),
                        Color32::from_rgb(70, 38, 44),
                        Stroke::new(1.0, Color32::from_rgb(116, 60, 72)),
                        Color32::from_rgb(255, 153, 166),
                        Color32::from_rgb(255, 228, 232),
                        Color32::from_rgb(255, 178, 187),
                    ),
                };

            ui.horizontal(|ui| {
                draw_chat_avatar(ui, avatar_label, avatar_fill);
                ui.add_space(8.0);
                egui::Frame::none()
                    .fill(bubble_fill)
                    .stroke(bubble_stroke)
                    .rounding(18.0)
                    .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(label)
                                    .size(10.5)
                                    .strong()
                                    .color(label_color),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(&message.timestamp)
                                        .size(10.5)
                                        .color(meta_color),
                                );
                            });
                        });
                        ui.add_space(4.0);
                        ui.label(RichText::new(visible_content).size(15.0).color(text_color));
                        if let Some(meta) = visible_meta {
                            ui.add_space(6.0);
                            ui.label(RichText::new(meta).size(11.0).color(meta_color));
                        }
                    });
            });
        });
    });
}

fn visible_chat_content(
    message: &ChatMessage,
    message_index: usize,
    chat_stream: Option<&ChatStreamState>,
) -> String {
    let Some(stream) = chat_stream else {
        return message.content.clone();
    };

    if stream.message_index != message_index {
        return message.content.clone();
    }

    message.content.chars().take(stream.revealed_chars).collect()
}

fn visible_chat_meta<'a>(
    message: &'a ChatMessage,
    message_index: usize,
    chat_stream: Option<&ChatStreamState>,
) -> Option<&'a str> {
    let Some(stream) = chat_stream else {
        return message.meta.as_deref();
    };

    if stream.message_index != message_index {
        return message.meta.as_deref();
    }

    if stream.revealed_chars >= stream.total_chars {
        message.meta.as_deref()
    } else {
        Some("streaming reply")
    }
}

fn draw_chat_avatar(ui: &mut egui::Ui, label: &str, fill: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(34.0, 34.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 17.0, fill);
    ui.painter().circle_stroke(
        rect.center(),
        17.0,
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 160),
        ),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        Color32::from_rgb(248, 250, 255),
    );
}

fn draw_chat_activity_bar(
    ui: &mut egui::Ui,
    animation_time: f32,
    accent: Color32,
    chat_running: bool,
    input_has_content: bool,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, Color32::from_rgb(27, 31, 39));

    let pulse = ((animation_time * if chat_running { 4.6 } else { 3.0 }).sin() * 0.5 + 0.5)
        .clamp(0.0, 1.0);
    let fill_ratio = if chat_running {
        0.45 + pulse * 0.35
    } else if input_has_content {
        0.18 + pulse * 0.18
    } else {
        0.10
    };
    let fill_rect = egui::Rect::from_min_size(
        rect.left_top(),
        Vec2::new(rect.width() * fill_ratio, rect.height()),
    );
    ui.painter().rect_filled(fill_rect, 3.0, accent);
}

fn draw_diagnostic_item(ui: &mut egui::Ui, item: &DiagnosticItem) {
    let (label, color) = match item.status {
        DiagnosticStatus::Pass => ("PASS", Color32::from_rgb(58, 201, 131)),
        DiagnosticStatus::Warn => ("WARN", Color32::from_rgb(247, 196, 83)),
        DiagnosticStatus::Fail => ("FAIL", Color32::from_rgb(236, 91, 110)),
    };

    ui.horizontal_wrapped(|ui| {
        status_pill(ui, label, color);
        ui.label(RichText::new(&item.label).strong().color(text_primary()));
        ui.label(RichText::new(&item.detail).color(text_secondary()));
    });
}

fn load_llm_config_summary(project_root: &Path) -> LlmConfigSummary {
    let source = resolve_companion_config_source(project_root);
    let config_file_exists = source.path.exists();
    let file_values = read_config_file_values(&source.path, source.format).unwrap_or_default();

    LlmConfigSummary {
        base_url: config_value_from_env_or_file(&file_values, "LLM_BASE_URL"),
        model: config_value_from_env_or_file(&file_values, "LLM_MODEL")
            .unwrap_or_else(|| "qwen-plus".to_string()),
        api_key_configured: config_value_from_env_or_file(&file_values, "LLM_API_KEY").is_some(),
        config_file_path: source.path,
        config_file_exists,
    }
}

fn resolve_companion_config_source(project_root: &Path) -> ConfigFileSource {
    if let Some(path) = std::env::var("COMPANION_CORE_CONFIG_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
    {
        return ConfigFileSource {
            format: detect_config_file_format(&path),
            path,
        };
    }

    if let Some(path) = std::env::var("COMPANION_CORE_ENV_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
    {
        return ConfigFileSource {
            format: ConfigFileFormat::Env,
            path,
        };
    }

    let toml_path = project_root
        .join("backend")
        .join("config")
        .join("companion-core.toml");
    if toml_path.exists() {
        return ConfigFileSource {
            format: ConfigFileFormat::Toml,
            path: toml_path,
        };
    }

    let env_path = project_root
        .join("backend")
        .join("config")
        .join("companion-core.env");
    if env_path.exists() {
        return ConfigFileSource {
            format: ConfigFileFormat::Env,
            path: env_path,
        };
    }

    ConfigFileSource {
        format: ConfigFileFormat::Toml,
        path: toml_path,
    }
}

fn detect_config_file_format(path: &Path) -> ConfigFileFormat {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("toml") => ConfigFileFormat::Toml,
        _ => ConfigFileFormat::Env,
    }
}

fn read_config_file_values(path: &Path, format: ConfigFileFormat) -> Option<Vec<(String, String)>> {
    match format {
        ConfigFileFormat::Toml => read_toml_file_values(path),
        ConfigFileFormat::Env => read_env_file_values(path),
    }
}

fn read_toml_file_values(path: &Path) -> Option<Vec<(String, String)>> {
    let content = fs::read_to_string(path).ok()?;
    let config = toml::from_str::<CompanionCoreTomlFile>(&content).ok()?;
    let llm = config.llm.unwrap_or_default();
    let mut values = Vec::new();

    insert_file_value(&mut values, "LLM_BASE_URL", llm.base_url);
    insert_file_value(&mut values, "LLM_MODEL", llm.model);
    insert_file_value(&mut values, "LLM_API_KEY", llm.api_key);

    Some(values)
}

fn read_env_file_values(path: &Path) -> Option<Vec<(String, String)>> {
    let content = fs::read_to_string(path).ok()?;
    let mut values = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }

        values.push((key.to_string(), unquote_env_value(value.trim()).to_string()));
    }

    Some(values)
}

fn insert_file_value(values: &mut Vec<(String, String)>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        values.push((key.to_string(), value));
    }
}

fn config_value_from_env_or_file(file_values: &[(String, String)], key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            file_values
                .iter()
                .find(|(file_key, _)| file_key == key)
                .map(|(_, value)| value.clone())
                .filter(|value| !value.trim().is_empty())
        })
}

fn unquote_env_value(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }

    value
}

fn post_companion_chat_message(session_id: &str, prompt: &str) -> Result<ChatCompletion, String> {
    let body = serde_json::json!({
        "session_id": session_id,
        "message": prompt,
        "persona": "Digital Girl"
    })
    .to_string();
    let addr = SocketAddr::from(([127, 0, 0, 1], 8787));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .map_err(|error| format!("connect to companion-core failed: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let request = format!(
        "POST /chat HTTP/1.1\r\nHost: 127.0.0.1:8787\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.as_bytes().len(),
        body
    );

    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("send test request failed: {error}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("read test response failed: {error}"))?;
    let Some((headers, response_body)) = response.split_once("\r\n\r\n") else {
        return Err("invalid HTTP response from companion-core".to_string());
    };
    let status_line = headers
        .lines()
        .next()
        .unwrap_or("HTTP response without status");
    if !status_line.contains(" 200 ") {
        return Err(format!(
            "{status_line}; {}",
            truncate_ui_detail(response_body, 500)
        ));
    }

    let value = serde_json::from_str::<Value>(response_body)
        .map_err(|error| format!("invalid JSON from companion-core: {error}"))?;
    let source = value
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let reply = value.get("reply").and_then(Value::as_str).unwrap_or("");
    let detail = value
        .get("detail")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());

    Ok(ChatCompletion {
        reply: reply.to_string(),
        source: source.to_string(),
        detail: detail.map(ToString::to_string),
    })
}

fn default_chat_messages(config: &LlmConfigSummary, session_id: &str) -> Vec<ChatMessage> {
    let intro = if config.is_ready() {
        format!(
            "The chat workspace is live. Messages will route through {} and keep context inside this thread.",
            config.model
        )
    } else {
        "The chat workspace is ready. Start companion-core and reload config when you want live model responses.".to_string()
    };

    vec![ChatMessage::assistant(
        intro,
        Some(format!(
            "{} • session {}",
            if config.is_ready() {
                config
                    .base_url
                    .as_deref()
                    .map(|value| truncate_ui_detail(value, 30))
                    .unwrap_or_else(|| "custom endpoint".to_string())
            } else {
                "local route".to_string()
            },
            compact_session_id(session_id)
        )),
    )]
}

fn chat_state_path(project_root: &Path) -> PathBuf {
    project_root
        .join("backend")
        .join("data")
        .join("companion-core-chat-state.json")
}

fn load_chat_state(path: &Path, config: &LlmConfigSummary) -> PersistedChatState {
    let loaded = fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<PersistedChatState>(&content).ok())
        .filter(|state| !state.session_id.trim().is_empty());

    match loaded {
        Some(mut state) => {
            if state.messages.is_empty() {
                state.messages = default_chat_messages(config, &state.session_id);
            }
            state
        }
        None => {
            let session_id = new_chat_session_id();
            PersistedChatState {
                messages: default_chat_messages(config, &session_id),
                session_id,
            }
        }
    }
}

fn write_chat_state(path: &Path, state: &PersistedChatState) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(content) = serde_json::to_vec_pretty(state) {
        let _ = fs::write(path, content);
    }
}

fn current_chat_timestamp() -> String {
    Local::now().format("%H:%M").to_string()
}

fn new_chat_session_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("egui-chat-{timestamp}")
}

fn compact_session_id(session_id: &str) -> String {
    if session_id.len() > 18 {
        format!("...{}", &session_id[session_id.len() - 12..])
    } else {
        session_id.to_string()
    }
}

fn format_chat_completion_meta(completion: &ChatCompletion) -> String {
    match completion.detail.as_deref() {
        Some(detail) => format!(
            "{} • {}",
            completion.source.to_uppercase(),
            truncate_ui_detail(detail, 120)
        ),
        None => completion.source.to_uppercase(),
    }
}

fn composer_accent_color(animation_time: f32, chat_running: bool, input_has_content: bool) -> Color32 {
    if chat_running {
        let pulse = ((animation_time * 4.6).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        blend_color(
            Color32::from_rgb(119, 128, 152),
            Color32::from_rgb(247, 196, 83),
            pulse,
        )
    } else if input_has_content {
        let pulse = ((animation_time * 3.0).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
        blend_color(
            Color32::from_rgb(96, 107, 130),
            Color32::from_rgb(88, 132, 255),
            0.55 + pulse * 0.35,
        )
    } else {
        Color32::from_rgb(49, 56, 70)
    }
}

fn blend_color(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let inv = 1.0 - amount;
    Color32::from_rgba_unmultiplied(
        (from.r() as f32 * inv + to.r() as f32 * amount).round() as u8,
        (from.g() as f32 * inv + to.g() as f32 * amount).round() as u8,
        (from.b() as f32 * inv + to.b() as f32 * amount).round() as u8,
        (from.a() as f32 * inv + to.a() as f32 * amount).round() as u8,
    )
}

fn truncate_ui_detail(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn collect_diagnostics(
    project_root: PathBuf,
    companion_dir: PathBuf,
    livetalking_dir: PathBuf,
    python_exe: PathBuf,
) -> Vec<DiagnosticItem> {
    let mut items = Vec::new();

    push_path_check(
        &mut items,
        "Project root",
        &project_root,
        "Expected repository root directory.",
    );
    push_path_check(
        &mut items,
        "companion-core crate",
        &companion_dir,
        "Rust sidecar and launcher crate.",
    );
    push_path_check(
        &mut items,
        "LiveTalking checkout",
        &livetalking_dir,
        "Python media runtime checkout.",
    );

    let venv_dir = project_root.join(".venv");
    push_path_check(
        &mut items,
        "Python virtual environment",
        &venv_dir,
        "Reused project environment.",
    );
    push_path_check(
        &mut items,
        "Python executable",
        &python_exe,
        "Interpreter selected by the control panel.",
    );

    check_python_version(&mut items, &python_exe);
    check_torch_stack(&mut items, &python_exe);
    check_livetalking_imports(&mut items, &python_exe);
    check_livetalking_assets(&mut items, &livetalking_dir);
    check_port(&mut items, 8787, "companion-core");
    check_port(&mut items, 8010, "LiveTalking");

    items
}

fn push_path_check(items: &mut Vec<DiagnosticItem>, label: &str, path: &Path, detail: &str) {
    if path.exists() {
        items.push(DiagnosticItem {
            status: DiagnosticStatus::Pass,
            label: label.to_string(),
            detail: format!("{} ({})", detail, path.display()),
        });
    } else {
        items.push(DiagnosticItem {
            status: DiagnosticStatus::Fail,
            label: label.to_string(),
            detail: format!("Missing: {}", path.display()),
        });
    }
}

fn check_python_version(items: &mut Vec<DiagnosticItem>, python_exe: &Path) {
    let mut command = Command::new(python_exe);
    command.arg("--version");
    match run_command_output(&mut command) {
        Ok(output) => items.push(DiagnosticItem {
            status: DiagnosticStatus::Pass,
            label: "Python version".to_string(),
            detail: output,
        }),
        Err(error) => items.push(DiagnosticItem {
            status: DiagnosticStatus::Fail,
            label: "Python version".to_string(),
            detail: error,
        }),
    }
}

fn check_torch_stack(items: &mut Vec<DiagnosticItem>, python_exe: &Path) {
    let script = r#"
import torch, torchvision, torchaudio
print(f"torch={torch.__version__}; torchvision={torchvision.__version__}; torchaudio={torchaudio.__version__}; cuda={torch.cuda.is_available()}; device={torch.cuda.get_device_name(0) if torch.cuda.is_available() else 'none'}")
"#;

    match run_python_script(python_exe, script) {
        Ok(output) => {
            let status = if output.contains("cuda=True") {
                DiagnosticStatus::Pass
            } else {
                DiagnosticStatus::Warn
            };
            items.push(DiagnosticItem {
                status,
                label: "PyTorch CUDA stack".to_string(),
                detail: output,
            });
        }
        Err(error) => items.push(DiagnosticItem {
            status: DiagnosticStatus::Fail,
            label: "PyTorch CUDA stack".to_string(),
            detail: error,
        }),
    }
}

fn check_livetalking_imports(items: &mut Vec<DiagnosticItem>, python_exe: &Path) {
    let script = r#"
import importlib.util as u
mods = ['flask','flask_sockets','aiortc','aiohttp_cors','cv2','onnxruntime','face_alignment','edge_tts','soundfile','librosa','numpy','scipy','numba','resampy','python_speech_features','configargparse','ffmpeg','openai','websockets']
missing = [m for m in mods if u.find_spec(m) is None]
print('missing=' + ','.join(missing) if missing else 'all required imports found')
raise SystemExit(1 if missing else 0)
"#;

    match run_python_script(python_exe, script) {
        Ok(output) => items.push(DiagnosticItem {
            status: DiagnosticStatus::Pass,
            label: "LiveTalking Python imports".to_string(),
            detail: output,
        }),
        Err(error) => items.push(DiagnosticItem {
            status: DiagnosticStatus::Fail,
            label: "LiveTalking Python imports".to_string(),
            detail: error,
        }),
    }
}

fn check_livetalking_assets(items: &mut Vec<DiagnosticItem>, livetalking_dir: &Path) {
    let model_path = livetalking_dir.join("models").join("wav2lip.pth");
    match fs::metadata(&model_path) {
        Ok(metadata) => items.push(DiagnosticItem {
            status: DiagnosticStatus::Pass,
            label: "Wav2Lip model".to_string(),
            detail: format!(
                "{} ({:.2} MB)",
                model_path.display(),
                metadata.len() as f64 / 1_048_576.0
            ),
        }),
        Err(_) => items.push(DiagnosticItem {
            status: DiagnosticStatus::Fail,
            label: "Wav2Lip model".to_string(),
            detail: format!("Missing: {}", model_path.display()),
        }),
    }

    let avatar_dir = livetalking_dir
        .join("data")
        .join("avatars")
        .join("wav2lip256_avatar1");
    let coords = avatar_dir.join("coords.pkl");
    let full_imgs = avatar_dir.join("full_imgs");
    let face_imgs = avatar_dir.join("face_imgs");

    let full_count = count_dir_entries(&full_imgs);
    let face_count = count_dir_entries(&face_imgs);
    let avatar_ok = coords.exists() && full_count.unwrap_or(0) > 0 && face_count.unwrap_or(0) > 0;

    items.push(DiagnosticItem {
        status: if avatar_ok {
            DiagnosticStatus::Pass
        } else {
            DiagnosticStatus::Fail
        },
        label: "Demo avatar assets".to_string(),
        detail: format!(
            "coords={}; full_imgs={}; face_imgs={}; path={}",
            if coords.exists() { "ok" } else { "missing" },
            full_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "missing".to_string()),
            face_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "missing".to_string()),
            avatar_dir.display()
        ),
    });
}

fn check_port(items: &mut Vec<DiagnosticItem>, port: u16, service_name: &str) {
    match get_port_owner(port) {
        Some(owner) => items.push(DiagnosticItem {
            status: DiagnosticStatus::Warn,
            label: format!("Port {port}"),
            detail: format!("{service_name} port is already listening: {owner}"),
        }),
        None => items.push(DiagnosticItem {
            status: DiagnosticStatus::Pass,
            label: format!("Port {port}"),
            detail: format!("Available for {service_name}."),
        }),
    }
}

fn count_dir_entries(path: &Path) -> Option<usize> {
    Some(fs::read_dir(path).ok()?.filter_map(Result::ok).count())
}

fn run_python_script(python_exe: &Path, script: &str) -> Result<String, String> {
    let mut command = Command::new(python_exe);
    command.arg("-c").arg(script);
    run_command_output(&mut command)
}

fn run_command_output(command: &mut Command) -> Result<String, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    hide_child_console(command);

    let output = command
        .output()
        .map_err(|error| format!("failed to run command: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let combined = match (stdout.is_empty(), stderr.is_empty()) {
        (false, false) => format!("{stdout}; {stderr}"),
        (false, true) => stdout,
        (true, false) => stderr,
        (true, true) => "command produced no output".to_string(),
    };

    if output.status.success() {
        Ok(combined)
    } else {
        Err(combined)
    }
}

fn get_port_owner(port: u16) -> Option<String> {
    let script = format!(
        "$c = Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1; if ($null -ne $c) {{ $p = Get-Process -Id $c.OwningProcess -ErrorAction SilentlyContinue; if ($null -ne $p) {{ Write-Output (\"PID={{0}} Name={{1}}\" -f $p.Id,$p.ProcessName) }} else {{ Write-Output (\"PID={{0}}\" -f $c.OwningProcess) }} }}"
    );
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(script);

    run_command_output(&mut command)
        .ok()
        .map(|output| output.trim().to_string())
        .filter(|output| !output.is_empty() && output != "command produced no output")
}

fn status_pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.horizontal(|ui| {
        status_dot(ui, color);
        ui.label(RichText::new(text).color(color).strong());
    });
}

fn spawn_server_process(
    mut command: Command,
    service: ServiceId,
    log_tx: Sender<LogEvent>,
    log_path: PathBuf,
    panel: &mut ServerPanel,
) -> Result<ServerProcess, String> {
    panel.last_exit = None;
    panel.push_log(format!("Starting: {}", panel.command_hint));
    append_log_file(&log_path, &format!("\n--- starting {} ---\n", panel.name));

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", panel.name))?;
    let pid = child.id();

    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(service, stdout, log_tx.clone(), log_path.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(service, stderr, log_tx, log_path.clone());
    }

    panel.push_log(format!("Process started with PID {pid}"));
    append_log_file(&log_path, &format!("Process started with PID {pid}\n"));
    Ok(ServerProcess { child, pid })
}

fn spawn_log_reader<R>(service: ServiceId, stream: R, log_tx: Sender<LogEvent>, log_path: PathBuf)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::new();

        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buffer)
                        .trim_end_matches(['\r', '\n'])
                        .to_string();
                    append_log_file(&log_path, &format!("{line}\n"));
                    let _ = log_tx.send(LogEvent { service, line });
                }
                Err(error) => {
                    append_log_file(&log_path, &format!("log read error: {error}\n"));
                    let _ = log_tx.send(LogEvent {
                        service,
                        line: format!("log read error: {error}"),
                    });
                    break;
                }
            }
        }
    });
}

fn append_log_file(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

fn stop_panel_process(panel: &mut ServerPanel) {
    if let Some(mut process) = panel.process.take() {
        panel.push_log(format!("Stopping process tree PID {}", process.pid));
        terminate_process_tree(process.pid);
        let _ = process.child.wait();
        panel.last_exit = Some("stopped by control panel".to_string());
    }
}

fn poll_panel_process(panel: &mut ServerPanel) {
    let Some(process) = panel.process.as_mut() else {
        return;
    };

    match process.child.try_wait() {
        Ok(Some(status)) => {
            panel.last_exit = Some(format!("exited with {status}"));
            panel.push_log(format!("Process exited with {status}"));
            panel.process = None;
        }
        Ok(None) => {}
        Err(error) => {
            panel.last_exit = Some(format!("process status error: {error}"));
            panel.push_log(format!("Process status error: {error}"));
            panel.process = None;
        }
    }
}

fn refresh_panel_port(panel: &mut ServerPanel) {
    if panel.last_probe.elapsed() < Duration::from_secs(1) {
        return;
    }

    panel.last_probe = Instant::now();
    panel.listening = is_port_open(panel.port);
}

fn is_port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
}

fn terminate_process_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn open_url(url: &str) {
    let _ = Command::new("cmd")
        .args(["/C", "start", "", url])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn configure_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    if let Some((font_name, font_bytes)) = load_cjk_font() {
        fonts
            .font_data
            .insert(font_name.clone(), FontData::from_owned(font_bytes));

        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .push(font_name.clone());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push(font_name);
    }

    ctx.set_fonts(fonts);
}

fn load_cjk_font() -> Option<(String, Vec<u8>)> {
    preferred_cjk_font_paths().into_iter().find_map(|path| {
        let font_name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .map(|name| format!("system-cjk-{name}"))?;
        let font_bytes = fs::read(&path).ok()?;
        Some((font_name, font_bytes))
    })
}

#[cfg(target_os = "windows")]
fn preferred_cjk_font_paths() -> Vec<PathBuf> {
    let windows_dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let font_dir = windows_dir.join("Fonts");

    vec![
        font_dir.join("Deng.ttf"),
        font_dir.join("simhei.ttf"),
        font_dir.join("simkai.ttf"),
    ]
}

#[cfg(target_os = "macos")]
fn preferred_cjk_font_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
        PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
    ]
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn preferred_cjk_font_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf"),
        PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc"),
    ]
}

fn configure_style(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(10.0, 10.0);
    style.spacing.button_padding = Vec2::new(12.0, 8.0);
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = app_bg();
    style.visuals.panel_fill = app_bg();
    style.visuals.faint_bg_color = panel_bg();
    style.visuals.extreme_bg_color = Color32::from_rgb(14, 16, 20);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(28, 31, 37);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(50, 55, 66));
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(33, 37, 44);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(64, 70, 82));
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(39, 44, 54);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(78, 86, 100));
    style.visuals.selection.bg_fill = Color32::from_rgb(70, 76, 92);
    style.visuals.override_text_color = Some(text_primary());
    style.visuals.window_rounding = 12.0.into();
    style.visuals.menu_rounding = 10.0.into();
    style.visuals.widgets.noninteractive.rounding = 10.0.into();
    style.visuals.widgets.inactive.rounding = 8.0.into();
    style.visuals.widgets.hovered.rounding = 8.0.into();
    style.visuals.widgets.active.rounding = 8.0.into();
    ctx.set_style(style);
}

#[cfg(windows)]
fn hide_child_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_child_console(_command: &mut Command) {}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Digital-Girl Control Center")
            .with_inner_size([1320.0, 860.0])
            .with_min_inner_size([1120.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Digital-Girl Control Center",
        native_options,
        Box::new(|cc| Box::new(ControlPanelApp::new(cc))),
    )
}
