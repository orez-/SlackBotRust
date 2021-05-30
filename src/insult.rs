use std::collections::HashMap;
use rand::seq::SliceRandom;
use rand::thread_rng;
use regex::Regex;
use rusoto_core::Region;
use rusoto_dynamodb::{AttributeValue, DynamoDb, DynamoDbClient, PutItemInput, ScanInput, ScanOutput};
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

enum PartOfSpeech {
    Noun,
    Adjective,
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
        let mut data = match item.get("word") {
            Some(AttributeValue { s: Some(string), .. }) => string.to_string(),
            _ => {
                discarded += 1;
                continue;
            },
        };
        match data.pop() {
            Some('n') => { nouns.push(data); }
            Some('a') => { adjectives.push(data); }
            _ => { discarded += 1; },
        }
    }
    if discarded > 0 {
        log::warn!("Discarding dynamodb insult words: {} words were malformed", discarded);
    }
    Ok(InsultFactory { nouns, adjectives })
}

async fn insert_word(word: String) -> LambdaResult<()> {
    let table_name = std::env::var("INSULT_TABLE")?;
    let mut item = HashMap::new();
    item.insert("word".to_string(), AttributeValue { s: Some(word), ..Default::default() });

    let client = DynamoDbClient::new(Region::UsEast1);
    let input = PutItemInput { item, table_name, ..Default::default() };
    client.put_item(input).await?;
    Ok(())
}

pub async fn handle_message(event: &MessageEvent) -> LambdaResult<()> {
    let re = Regex::new(r"\binsult\s+(<@U\w+>)$").unwrap();
    if let Some(caps) = re.captures(&event.text) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return handle_say_insult(event, name).await;
    }

    let re = Regex::new(r"\binsult\s+me$").unwrap();
    if re.is_match(&event.text) {
        return handle_say_insult(event, to_user_tag(event.user.as_str())).await;
    }

    let re = Regex::new(r"^(?:<@U\w+>\s)?\s*add\s+(adjective|noun)\s+([\w ,-]+)$").unwrap();
    if let Some(caps) = re.captures(&event.text) {
        let pos = match caps.get(1).unwrap().as_str() {
            "adjective" => PartOfSpeech::Adjective,
            "noun" => PartOfSpeech::Noun,
            _ => unreachable!(),
        };
        let insult = caps.get(2).unwrap().as_str().trim().to_string();
        if insult == "" {
            return send_message(&event.channel, "Nice try wise guy.").await;
        }
        return handle_add_word(&event, pos, insult).await;
    }
    Ok(())
}

async fn handle_say_insult(event: &MessageEvent, user_tag: String) -> LambdaResult<()> {
    let insults = insult_factory().await?;
    let message = match insults.get_insult() {
        Some(insult) => format!("{} is {}", user_tag, insult),
        None => "Shut up.".to_string(),
    };

    send_message(&event.channel, &message).await
}

async fn handle_add_word(event: &MessageEvent, pos: PartOfSpeech, mut insult: String) -> LambdaResult<()> {
    let c = match pos {
        PartOfSpeech::Noun => 'n',
        PartOfSpeech::Adjective => 'a',
    };
    insult.push(c);
    insert_word(insult).await?;
    send_message(&event.channel, "Added.").await
}
