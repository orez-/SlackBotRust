use rand::thread_rng;
use rand::seq::SliceRandom;

use crate::{MessageEvent, send_message};


pub async fn insult(event: &MessageEvent) {
    let mut adjectives = Vec::new();
    adjectives.push("poopy".to_string());
    adjectives.push("bad".to_string());

    let mut nouns = Vec::new();
    nouns.push("butthole".to_string());
    nouns.push("jerk".to_string());

    let adjective = adjectives.choose(&mut thread_rng()).unwrap();
    let noun = nouns.choose(&mut thread_rng()).unwrap();

    let first_name = event.user.as_str();

    send_message(
        &event.channel,
        &format!("{} is a {} {}", first_name, adjective, noun)
    ).await;
}
