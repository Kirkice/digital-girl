use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, fs, net::SocketAddr, path::{Path, PathBuf}, sync::Arc, time::Duration};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    persona: Arc<Persona>,
    llm: Arc<LlmConfig>,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

#[derive(Clone, Debug, Default)]
struct RuntimeConfig {
    values: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug)]
enum ConfigFileFormat {
    Toml,
    Env,
}

#[derive(Debug)]
struct ConfigFileSource {
    path: PathBuf,
    format: ConfigFileFormat,
}

#[derive(Debug, Default, Deserialize)]
struct CompanionCoreTomlConfig {
    server: Option<ServerTomlConfig>,
    persona: Option<PersonaTomlConfig>,
    llm: Option<LlmTomlConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct ServerTomlConfig {
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Default, Deserialize)]
struct PersonaTomlConfig {
    name: Option<String>,
    summary: Option<String>,
    system_prompt: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct LlmTomlConfig {
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Serialize)]
struct Persona {
    name: String,
    summary: String,
    system_prompt: String,
}

#[derive(Clone, Debug)]
struct LlmConfig {
    base_url: Option<String>,
    api_key: Option<String>,
    model: String,
}

impl LlmConfig {
    fn is_configured(&self) -> bool {
        self.base_url.is_some() && self.api_key.is_some()
    }

    fn status(&self) -> LlmStatusResponse {
        LlmStatusResponse {
            configured: self.is_configured(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            api_key_configured: self.api_key.is_some(),
        }
    }
}

#[derive(Debug, Serialize)]
struct LlmStatusResponse {
    configured: bool,
    base_url: Option<String>,
    model: String,
    api_key_configured: bool,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    session_id: String,
    message: String,
    persona: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChatResponse {
    session_id: String,
    reply: String,
    source: &'static str,
    detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiReplyMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiReplyMessage {
    content: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "companion_core=info,tower_http=info".into()),
        )
        .init();

    let runtime_config = load_runtime_config();
    let state = AppState {
        persona: Arc::new(load_persona(&runtime_config)),
        llm: Arc::new(load_llm_config(&runtime_config)),
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()?,
    };

    info!(
        llm_configured = state.llm.is_configured(),
        llm_model = %state.llm.model,
        "companion-core runtime config loaded"
    );

    let app = Router::new()
        .route("/health", get(health))
        .route("/persona", get(persona))
        .route("/llm/status", get(llm_status))
        .route("/chat", post(chat))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let host = config_value(&runtime_config, "COMPANION_CORE_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let port = config_value(&runtime_config, "COMPANION_CORE_PORT")
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8787);
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "companion-core listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "companion-core",
    })
}

async fn persona(State(state): State<AppState>) -> Json<Persona> {
    Json(Persona {
        name: state.persona.name.clone(),
        summary: state.persona.summary.clone(),
        system_prompt: state.persona.system_prompt.clone(),
    })
}

async fn llm_status(State(state): State<AppState>) -> Json<LlmStatusResponse> {
    Json(state.llm.status())
}

async fn chat(State(state): State<AppState>, Json(request): Json<ChatRequest>) -> impl IntoResponse {
    if request.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ChatResponse {
                session_id: request.session_id,
                reply: "message must not be empty".to_string(),
                source: "validation",
                detail: None,
            }),
        );
    }

    let mut source = "local";
    let mut detail = if state.llm.is_configured() {
        None
    } else {
        Some("llm is not configured; using local placeholder reply".to_string())
    };

    if state.llm.is_configured() {
        match call_openai_compatible(&state, &request).await {
            Ok(reply) => {
                return (
                    StatusCode::OK,
                    Json(ChatResponse {
                        session_id: request.session_id,
                        reply,
                        source: "llm",
                        detail: None,
                    }),
                );
            }
            Err(error) => {
                source = "fallback";
                detail = Some(format!("llm request failed; using local fallback: {error}"));
                warn!(?error, "llm request failed; falling back to local reply");
            }
        }
    }

    (
        StatusCode::OK,
        Json(ChatResponse {
            session_id: request.session_id.clone(),
            reply: local_reply(&state.persona, &request),
            source,
            detail,
        }),
    )
}

fn load_persona(config: &RuntimeConfig) -> Persona {
    Persona {
        name: config_value(config, "DIGITAL_GIRL_PERSONA_NAME").unwrap_or_else(|| "Digital Girl".to_string()),
        summary: config_value(config, "DIGITAL_GIRL_PERSONA_SUMMARY")
            .unwrap_or_else(|| "A warm, playful, private AI companion.".to_string()),
        system_prompt: config_value(config, "DIGITAL_GIRL_SYSTEM_PROMPT").unwrap_or_else(|| {
            "You are a warm, concise, emotionally attentive companion. Reply naturally and avoid long lectures."
                .to_string()
        }),
    }
}

fn load_llm_config(config: &RuntimeConfig) -> LlmConfig {
    LlmConfig {
        base_url: config_value(config, "LLM_BASE_URL"),
        api_key: config_value(config, "LLM_API_KEY"),
        model: config_value(config, "LLM_MODEL").unwrap_or_else(|| "qwen-plus".to_string()),
    }
}

fn local_reply(persona: &Persona, request: &ChatRequest) -> String {
    let persona_name = request.persona.as_deref().unwrap_or(&persona.name);
    format!("{persona_name}: 我收到啦。你刚才说：{}", request.message.trim())
}

async fn call_openai_compatible(state: &AppState, request: &ChatRequest) -> anyhow::Result<String> {
    let base_url = state.llm.base_url.as_ref().expect("checked by caller");
    let api_key = state.llm.api_key.as_ref().expect("checked by caller");
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let payload = OpenAiRequest {
        model: state.llm.model.clone(),
        messages: vec![
            OpenAiMessage {
                role: "system".to_string(),
                content: state.persona.system_prompt.clone(),
            },
            OpenAiMessage {
                role: "user".to_string(),
                content: request.message.clone(),
            },
        ],
        temperature: 0.8,
    };

    let response = state
        .client
        .post(url)
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("provider request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| anyhow::anyhow!("failed to read provider response: {error}"))?;

    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "provider returned HTTP {status}: {}",
            truncate_detail(&body, 500)
        ));
    }

    let response = serde_json::from_str::<OpenAiResponse>(&body).map_err(|error| {
        anyhow::anyhow!(
            "malformed provider response: {error}; body={}",
            truncate_detail(&body, 500)
        )
    })?;

    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("empty llm response"))
}

fn load_runtime_config() -> RuntimeConfig {
    let Some(source) = resolve_runtime_config_source() else {
        return RuntimeConfig::default();
    };

    let path = source.path;

    if !path.exists() {
        info!(path = %path.display(), format = ?source.format, "companion-core config file not found; using process environment only");
        return RuntimeConfig {
            values: HashMap::new(),
        };
    }

    let parsed = match source.format {
        ConfigFileFormat::Toml => parse_toml_file(&path),
        ConfigFileFormat::Env => parse_env_file(&path),
    };

    match parsed {
        Ok(values) => {
            info!(path = %path.display(), format = ?source.format, "loaded companion-core config file");
            RuntimeConfig {
                values,
            }
        }
        Err(error) => {
            warn!(?error, path = %path.display(), format = ?source.format, "failed to load companion-core config file; using process environment only");
            RuntimeConfig {
                values: HashMap::new(),
            }
        }
    }
}

fn resolve_runtime_config_source() -> Option<ConfigFileSource> {
    if let Some(path) = env::var("COMPANION_CORE_CONFIG_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
    {
        return Some(ConfigFileSource {
            format: detect_config_file_format(&path),
            path,
        });
    }

    if let Some(path) = env::var("COMPANION_CORE_ENV_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
    {
        return Some(ConfigFileSource {
            format: ConfigFileFormat::Env,
            path,
        });
    }

    let root = project_root()?;
    let toml_path = root.join("backend").join("config").join("companion-core.toml");
    if toml_path.exists() {
        return Some(ConfigFileSource {
            format: ConfigFileFormat::Toml,
            path: toml_path,
        });
    }

    let env_path = root.join("backend").join("config").join("companion-core.env");
    if env_path.exists() {
        return Some(ConfigFileSource {
            format: ConfigFileFormat::Env,
            path: env_path,
        });
    }

    Some(ConfigFileSource {
        format: ConfigFileFormat::Toml,
        path: toml_path,
    })
}

fn detect_config_file_format(path: &Path) -> ConfigFileFormat {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("toml") => ConfigFileFormat::Toml,
        _ => ConfigFileFormat::Env,
    }
}

fn project_root() -> Option<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

fn config_value(config: &RuntimeConfig, key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| config.values.get(key).cloned().filter(|value| !value.trim().is_empty()))
}

fn parse_toml_file(path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let content = fs::read_to_string(path)?;
    let config = toml::from_str::<CompanionCoreTomlConfig>(&content)?;
    let mut values = HashMap::new();

    if let Some(server) = config.server {
        insert_config_value(&mut values, "COMPANION_CORE_HOST", server.host);
        insert_config_value(
            &mut values,
            "COMPANION_CORE_PORT",
            server.port.map(|value| value.to_string()),
        );
    }

    if let Some(persona) = config.persona {
        insert_config_value(&mut values, "DIGITAL_GIRL_PERSONA_NAME", persona.name);
        insert_config_value(&mut values, "DIGITAL_GIRL_PERSONA_SUMMARY", persona.summary);
        insert_config_value(
            &mut values,
            "DIGITAL_GIRL_SYSTEM_PROMPT",
            persona.system_prompt,
        );
    }

    if let Some(llm) = config.llm {
        insert_config_value(&mut values, "LLM_BASE_URL", llm.base_url);
        insert_config_value(&mut values, "LLM_API_KEY", llm.api_key);
        insert_config_value(&mut values, "LLM_MODEL", llm.model);
    }

    Ok(values)
}

fn parse_env_file(path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let content = fs::read_to_string(path)?;
    let mut values = HashMap::new();

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

        values.insert(key.to_string(), unquote_env_value(value.trim()).to_string());
    }

    Ok(values)
}

fn insert_config_value(values: &mut HashMap<String, String>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        values.insert(key.to_string(), value);
    }
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

fn truncate_detail(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
