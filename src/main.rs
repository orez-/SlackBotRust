use hyper::{Body, Client, Request, Method};
use hyper_openssl::{HttpsConnector, HttpsLayer};
use lambda_runtime::{handler_fn, Context, Error as LambdaError};
use log::LevelFilter;
use openssl::ssl::{SslConnector, SslMethod};
use serde::de::Error as _;
use serde::{Serialize, Deserialize, Deserializer};
use serde_json::{json, Value};
use simple_logger::SimpleLogger;
use std::collections::HashMap;
use std::env;
use std::error::Error;

mod insult;

#[tokio::main]
async fn main() -> Result<(), LambdaError> {
    SimpleLogger::new().with_level(LevelFilter::Info).init().unwrap();
    openssl_probe::init_ssl_cert_env_vars();

    let func = handler_fn(api_gateway_func);
    lambda_runtime::run(func).await?;
    Ok(())
}

// https://docs.aws.amazon.com/lambda/latest/dg/services-apigateway.html
#[derive(Deserialize)]
struct ApiGatewayEvent {
    #[serde(deserialize_with = "deserialize_str")]
    body: Value
}

// Deserializer for a json encoded string.
// Ex: json!("\"5\"") -> 5
fn deserialize_str<'de, D>(deserializer: D) -> Result<Value, D::Error> where D: Deserializer<'de> {
    let s: String = Deserialize::deserialize(deserializer)?;
    serde_json::from_str(&s).map_err(D::Error::custom)
}

#[derive(Serialize)]
struct ApiGatewayResponse {
    statusCode: u16,
    headers: HashMap<String, String>,
    body: String,
}

impl ApiGatewayResponse {
    fn ok(body: Value) -> Self {
        let body = body.to_string();
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        ApiGatewayResponse { statusCode: 200, headers, body }
    }
}

#[derive(Deserialize)]
struct ChallengeEvent {
    token: String,
    challenge: String,
}


#[derive(Deserialize, Debug)]
pub struct MessageEvent {
    subtype: Option<String>,
    channel: String,
    user: String,
    text: String,
    ts: String,
}

#[derive(Deserialize, Debug)]
pub struct CallbackEvent {
    #[serde(deserialize_with = "deserialize_event_callback")]
    event: EventType,
}

#[derive(Debug)]
enum EventType {
    Message(MessageEvent),
    Unsupported,
}

fn deserialize_event_callback<'de, D>(deserializer: D) -> Result<EventType, D::Error> where D: Deserializer<'de> {
    let s: Value = Deserialize::deserialize(deserializer)?;
    let type_ = match s.get("type") {
        Some(Value::String(t)) => t,
        Some(_) => return Err(D::Error::custom("expected string for field 'type'")),
        None => return Err(D::Error::custom("slack event missing field 'type'")),
    };
    Ok(match type_.as_str() {
        "message" | "app_mention" =>
            EventType::Message(serde_json::from_value(s).map_err(D::Error::custom)?),
        _ => EventType::Unsupported,
    })
}

// https://api.slack.com/events/url_verification
fn respond_to_challenge(event: Value) -> Result<Value, LambdaError> {
    let event: ChallengeEvent = serde_json::from_value(event)?;
    Ok(json!({ "challenge": event.challenge }))
}

async fn handle_event_callback(event: Value) -> Result<(), LambdaError> {
    let event: CallbackEvent = serde_json::from_value(event)?;
    log::info!("Event callback event {:?}", event);
    match event.event {
        EventType::Message(mevent) if mevent.text.contains("insult me") => {
            insult::insult(&mevent).await;
        },
        _ => (),
    }
    Ok(())
}

pub async fn send_message(channel: &str, message: &str) {
    match _send_message(channel, message).await {
        Err(e) => { log::error!("Error sending message: {}", e); }
        _ => (),
    };
}

async fn _send_message(channel: &str, message: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let token = env::var("SLACK_TOKEN")?;
    let https = HttpsConnector::new()?;
    let client: Client<_, Body> = Client::builder()
        // Fix "connection closed before message completed" errors.
        // https://github.com/wyyerd/stripe-rs/pull/172
        .pool_max_idle_per_host(0)
        .build(https);

    let request = Request::builder()
        .method(Method::POST)
        .uri("https://slack.com/api/chat.postMessage/")
        .header("content-type", "application/json; charset=utf-8")
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::from(json!({
            "text": message,
            "channel": channel,
        }).to_string()))?;

    client.request(request).await?;
    Ok(())
}

async fn route_request(event: ApiGatewayEvent) -> Result<Value, LambdaError> {
    let ApiGatewayEvent {body, ..} = event;
    let type_ = match body.get("type") {
        Some(Value::String(t)) => t,
        Some(_) => return Ok(json!({"error": "expected string for field 'type'"})),
        None => return Ok(json!({"error": "slack event missing field 'type'"})),
    };
    log::info!("Payload body: {:?}", body);
    match type_.as_str() {
        "url_verification" => { return respond_to_challenge(body); },
        "event_callback" => { handle_event_callback(body).await?; },
        _ => (),
    };

    Ok(json!( { "ok": true } ))
}

async fn api_gateway_func(event: Value, _: Context) -> Result<Value, LambdaError> {
    let event: ApiGatewayEvent = serde_json::from_value(event)?;
    let body = route_request(event).await?;
    Ok(serde_json::to_value(ApiGatewayResponse::ok(body))?)
}
