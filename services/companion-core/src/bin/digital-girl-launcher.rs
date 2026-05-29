use eframe::egui::{
    self, Align, Color32, Context, FontId, Layout, RichText, ScrollArea, Stroke, Vec2,
};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
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

struct ControlPanelApp {
    project_root: PathBuf,
    companion_dir: PathBuf,
    livetalking_dir: PathBuf,
    python_exe: PathBuf,
    companion: ServerPanel,
    livetalking: ServerPanel,
    log_tx: Sender<LogEvent>,
    log_rx: Receiver<LogEvent>,
    last_error: Option<String>,
}

impl ControlPanelApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_style(&cc.egui_ctx);

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

        Self {
            project_root,
            companion_dir,
            livetalking_dir,
            python_exe,
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
            last_error: None,
        }
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
            &mut self.livetalking,
        )?;
        self.livetalking.process = Some(process);
        Ok(())
    }

    fn stop_service(&mut self, service: ServiceId) {
        let panel = self.panel_mut(service);
        stop_panel_process(panel);
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
        self.stop_all();
    }
}

impl eframe::App for ControlPanelApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.drain_logs();
        self.poll_processes();
        self.refresh_ports();

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Digital-Girl Control Panel")
                            .font(FontId::proportional(26.0))
                            .color(Color32::from_rgb(238, 244, 255)),
                    );
                    ui.label(
                        RichText::new("Launch, monitor, and stop the local digital human stack")
                            .color(Color32::from_rgb(150, 162, 180)),
                    );
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Open Digital Human").clicked() {
                        open_url(&self.livetalking.url);
                    }
                    if ui.button("Stop All").clicked() {
                        self.stop_all();
                    }
                    if ui.button("Start All").clicked() {
                        self.start_service(ServiceId::CompanionCore);
                        self.start_service(ServiceId::LiveTalking);
                    }
                });
            });
            ui.add_space(10.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(error) = &self.last_error {
                egui::Frame::none()
                    .fill(Color32::from_rgb(70, 24, 31))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(170, 70, 84)))
                    .rounding(8.0)
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new(error).color(Color32::from_rgb(255, 203, 211)));
                    });
                ui.add_space(12.0);
            }

            let mut actions = Vec::new();
            ui.columns(2, |columns| {
                if let Some(action) = draw_server_card(&mut columns[0], &mut self.companion) {
                    actions.push(action);
                }
                if let Some(action) = draw_server_card(&mut columns[1], &mut self.livetalking) {
                    actions.push(action);
                }
            });

            for action in actions {
                match action {
                    UiAction::Start(service) => self.start_service(service),
                    UiAction::Stop(service) => self.stop_service(service),
                }
            }

            ui.add_space(14.0);
            egui::Frame::none()
                .fill(Color32::from_rgb(14, 18, 24))
                .stroke(Stroke::new(1.0, Color32::from_rgb(39, 48, 60)))
                .rounding(8.0)
                .inner_margin(14.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Runtime Paths")
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.add_space(8.0);
                    ui.monospace(format!("Project       {}", self.project_root.display()));
                    ui.monospace(format!("LiveTalking   {}", self.livetalking_dir.display()));
                    ui.monospace(format!("Python        {}", self.python_exe.display()));
                });
        });

        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

fn draw_server_card(ui: &mut egui::Ui, panel: &mut ServerPanel) -> Option<UiAction> {
    let mut action = None;

    ui.push_id(panel.name, |ui| {
        egui::Frame::none()
            .fill(Color32::from_rgb(18, 23, 31))
            .stroke(Stroke::new(1.0, Color32::from_rgb(45, 56, 70)))
            .rounding(10.0)
            .inner_margin(14.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(panel.name)
                                .font(FontId::proportional(20.0))
                                .color(Color32::from_rgb(238, 244, 255)),
                        );
                        ui.label(
                            RichText::new(panel.description)
                                .color(Color32::from_rgb(145, 157, 176)),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        status_pill(ui, panel.status_label(), panel.status_color());
                    });
                });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let start_enabled = !panel.is_running();
                    if ui
                        .add_enabled(
                            start_enabled,
                            egui::Button::new("Start").min_size(Vec2::new(78.0, 30.0)),
                        )
                        .clicked()
                    {
                        action = Some(UiAction::Start(panel.service));
                    }
                    let stop_enabled = panel.is_running();
                    if ui
                        .add_enabled(
                            stop_enabled,
                            egui::Button::new("Stop").min_size(Vec2::new(78.0, 30.0)),
                        )
                        .clicked()
                    {
                        action = Some(UiAction::Stop(panel.service));
                    }
                    if ui.button("Open URL").clicked() {
                        open_url(&panel.url);
                    }
                });

                ui.add_space(10.0);
                ui.monospace(format!("URL      {}", panel.url));
                ui.monospace(format!("Command  {}", panel.command_hint));
                if let Some(exit) = &panel.last_exit {
                    ui.monospace(format!("Last     {exit}"));
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Logs")
                        .strong()
                        .color(Color32::from_rgb(210, 220, 235)),
                );
                ScrollArea::vertical()
                    .max_height(300.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if panel.logs.is_empty() {
                            ui.label(
                                RichText::new(
                                    "No logs yet. Start the service to stream output here.",
                                )
                                .color(Color32::from_rgb(120, 132, 150)),
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

fn status_pill(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(Color32::from_rgba_unmultiplied(
            color.r(),
            color.g(),
            color.b(),
            30,
        ))
        .stroke(Stroke::new(1.0, color))
        .rounding(999.0)
        .inner_margin(egui::Margin::symmetric(10.0, 5.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).strong());
        });
}

fn spawn_server_process(
    mut command: Command,
    service: ServiceId,
    log_tx: Sender<LogEvent>,
    panel: &mut ServerPanel,
) -> Result<ServerProcess, String> {
    panel.last_exit = None;
    panel.push_log(format!("Starting: {}", panel.command_hint));

    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to start {}: {error}", panel.name))?;
    let pid = child.id();

    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(service, stdout, log_tx.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(service, stderr, log_tx);
    }

    panel.push_log(format!("Process started with PID {pid}"));
    Ok(ServerProcess { child, pid })
}

fn spawn_log_reader<R>(service: ServiceId, stream: R, log_tx: Sender<LogEvent>)
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
                    let _ = log_tx.send(LogEvent { service, line });
                }
                Err(error) => {
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

fn configure_style(ctx: &Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = Color32::from_rgb(9, 12, 17);
    visuals.panel_fill = Color32::from_rgb(9, 12, 17);
    visuals.faint_bg_color = Color32::from_rgb(18, 23, 31);
    visuals.widgets.active.bg_fill = Color32::from_rgb(55, 119, 255);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 88, 160);
    ctx.set_visuals(visuals);
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
            .with_title("Digital-Girl Control Panel")
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([920.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Digital-Girl Control Panel",
        native_options,
        Box::new(|cc| Box::new(ControlPanelApp::new(cc))),
    )
}
