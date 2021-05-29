use rand::thread_rng;
use rand::seq::SliceRandom;

use crate::{MessageEvent, send_message};


fn to_user_tag(user_id: &str) -> String {
    format!("<@{}>", user_id).to_string()
}

pub fn is_insult_request(event: &MessageEvent) -> bool {
    event.text.contains("insult me")
}

pub async fn insult(event: &MessageEvent) {
    let mut adjectives = Vec::new();
    adjectives.push("awful".to_string());
    adjectives.push("poopy".to_string());
    adjectives.push("bad".to_string());

    let mut nouns = Vec::new();
    nouns.push("butthole".to_string());
    nouns.push("jerk".to_string());

    let adjective = adjectives.choose(&mut thread_rng()).unwrap();
    let noun = nouns.choose(&mut thread_rng()).unwrap();
    let article = if let Some(chr) = adjective.chars().next() {
        match chr {
            'a' | 'e' | 'i' | 'o' | 'u' |
            'A' | 'E' | 'I' | 'O' | 'U' => "an",
            _ => "a",
        }
    } else { "a" };

    let user_tag = to_user_tag(event.user.as_str());

    send_message(
        &event.channel,
        &format!("{} is {} {} {}", user_tag, article, adjective, noun)
    ).await;
}
