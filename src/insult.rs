use rand::seq::SliceRandom;
use rand::thread_rng;
use rusoto_core::Region;
use rusoto_dynamodb::{AttributeValue, DynamoDb, DynamoDbClient, ScanInput, ScanOutput};
use tokio::sync::OnceCell;

use crate::{send_message, LambdaResult, MessageEvent};

async fn insult_factory() -> LambdaResult<&'static InsultFactory> {
    static INSTANCE: OnceCell<InsultFactory> = OnceCell::const_new();
    INSTANCE.get_or_try_init(fetch_insults).await
}

struct InsultFactory {
    nouns: Vec<String>,
    adjectives: Vec<String>,
}

impl InsultFactory {
    fn get_insult(&self) -> Option<String> {
        let adjective = self.adjectives.choose(&mut thread_rng())?;
        let noun = self.nouns.choose(&mut thread_rng())?;
        let article = if let Some(chr) = adjective.chars().next() {
            match chr {
                'a' | 'e' | 'i' | 'o' | 'u' |
                'A' | 'E' | 'I' | 'O' | 'U' => "an",
                _ => "a",
            }
        } else { "a" };

        Some(format!("{} {} {}", article, adjective, noun))
    }
}

fn to_user_tag(user_id: &str) -> String {
    format!("<@{}>", user_id)
}

async fn fetch_insults() -> LambdaResult<InsultFactory> {
    let table_name = std::env::var("INSULT_TABLE")?;
    let client = DynamoDbClient::new(Region::UsEast1);
    let input = ScanInput { table_name, ..Default::default() };
    let ScanOutput { items, .. } = client.scan(input).await?;
    let items = items.unwrap();  // when would this happen??

    let mut nouns = Vec::new();
    let mut adjectives = Vec::new();
    let mut discarded = 0;
    for item in items {
        let word = match item.get("word") {
            Some(AttributeValue { s: Some(string), .. }) => string.to_string(),
            _ => {
                discarded += 1;
                continue;
            },
        };
        match item.get("isNoun") {
            Some(AttributeValue { bool: Some(true), .. }) => { nouns.push(word); },
            Some(AttributeValue { bool: Some(false), .. }) => { adjectives.push(word); },
            _ => { discarded += 1; },
        }
    }
    if discarded > 0 {
        log::warn!("Discarding dynamodb insult words: {} words were malformed", discarded);
    }
    Ok(InsultFactory { nouns, adjectives })
}

pub async fn handle_message(event: &MessageEvent) -> LambdaResult<()> {
    if event.text.contains("insult me") {
        return insult(event).await;
    }
    Ok(())
}

async fn insult(event: &MessageEvent) -> LambdaResult<()> {
    let insults = insult_factory().await?;
    let user_tag = to_user_tag(event.user.as_str());
    let message = match insults.get_insult() {
        Some(insult) => format!("{} is {}", user_tag, insult),
        None => "Shut up.".to_string(),
    };

    send_message(&event.channel, &message).await;
    Ok(())
}

async fn add_word(event: &MessageEvent) {
    // @bot add (adjective|noun) ([\w ,-]+)$
    send_message(&event.channel, "Added.").await;
}
