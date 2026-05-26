use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{env, net::SocketAddr, sync::Arc, time::Duration};
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

    let state = AppState {
        persona: Arc::new(load_persona()),
        llm: Arc::new(load_llm_config()),
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(45))
            .build()?,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/persona", get(persona))
        .route("/chat", post(chat))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let host = env::var("COMPANION_CORE_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("COMPANION_CORE_PORT")
        .ok()
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

async fn chat(State(state): State<AppState>, Json(request): Json<ChatRequest>) -> impl IntoResponse {
    if request.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ChatResponse {
                session_id: request.session_id,
                reply: "message must not be empty".to_string(),
                source: "validation",
            }),
        );
    }

    if state.llm.base_url.is_some() && state.llm.api_key.is_some() {
        match call_openai_compatible(&state, &request).await {
            Ok(reply) => {
                return (
                    StatusCode::OK,
                    Json(ChatResponse {
                        session_id: request.session_id,
                        reply,
                        source: "llm",
                    }),
                );
            }
            Err(error) => {
                warn!(?error, "llm request failed; falling back to local reply");
            }
        }
    }

    (
        StatusCode::OK,
        Json(ChatResponse {
            session_id: request.session_id.clone(),
            reply: local_reply(&state.persona, &request),
            source: "local",
        }),
    )
}

fn load_persona() -> Persona {
    Persona {
        name: env::var("DIGITAL_GIRL_PERSONA_NAME").unwrap_or_else(|_| "Digital Girl".to_string()),
        summary: env::var("DIGITAL_GIRL_PERSONA_SUMMARY")
            .unwrap_or_else(|_| "A warm, playful, private AI companion.".to_string()),
        system_prompt: env::var("DIGITAL_GIRL_SYSTEM_PROMPT").unwrap_or_else(|_| {
            "You are a warm, concise, emotionally attentive companion. Reply naturally and avoid long lectures."
                .to_string()
        }),
    }
}

fn load_llm_config() -> LlmConfig {
    LlmConfig {
        base_url: env::var("LLM_BASE_URL").ok().filter(|value| !value.trim().is_empty()),
        api_key: env::var("LLM_API_KEY").ok().filter(|value| !value.trim().is_empty()),
        model: env::var("LLM_MODEL").unwrap_or_else(|_| "qwen-plus".to_string()),
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
        .await?
        .error_for_status()?
        .json::<OpenAiResponse>()
        .await?;

    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("empty llm response"))
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
