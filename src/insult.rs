use std::collections::HashMap;
use std::sync::RwLock;
use rand::seq::SliceRandom;
use rand::thread_rng;
use regex::Regex;
use rusoto_core::Region;
use rusoto_dynamodb::{AttributeValue, DynamoDb, DynamoDbClient, PutItemInput, ScanInput, ScanOutput};
use tokio::sync::OnceCell;

use crate::{send_message, LambdaResult, MessageEvent};

async fn insult_factory() -> LambdaResult<&'static RwLock<InsultFactory>> {
    static INSTANCE: OnceCell<RwLock<InsultFactory>> = OnceCell::const_new();
    INSTANCE.get_or_try_init(fetch_insults_rw).await
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

    fn insert_word(&mut self, pos: &PartOfSpeech, word: String) -> bool {
        let list = match pos {
            PartOfSpeech::Noun => &mut self.nouns,
            PartOfSpeech::Adjective => &mut self.adjectives,
        };
        if list.iter().any(|w| w == &word) {
            return false;
        }
        list.push(word);
        true
    }
}

#[derive(Debug)]
struct GenericError(String);
impl std::error::Error for GenericError {}
impl std::fmt::Display for GenericError {
    fn fmt(&self, fmtr: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        fmtr.write_fmt(format_args!("GenericError({})", self.0))
    }
}

enum PartOfSpeech {
    Noun,
    Adjective,
}

fn to_user_tag(user_id: &str) -> String {
    format!("<@{}>", user_id)
}

async fn fetch_insults_rw() -> LambdaResult<RwLock<InsultFactory>> {
    fetch_insults().await.map(RwLock::new)
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

async fn insert_word_to_dynamo(word: String) -> LambdaResult<()> {
    let table_name = std::env::var("INSULT_TABLE")?;
    let mut item = HashMap::new();
    item.insert("word".to_string(), AttributeValue { s: Some(word), ..Default::default() });

    let client = DynamoDbClient::new(Region::UsEast1);
    let input = PutItemInput { item, table_name, ..Default::default() };
    client.put_item(input).await?;
    Ok(())
}

pub async fn handle_message(event: &MessageEvent) -> LambdaResult<()> {
    let re = Regex::new(r"\binsult\s+(<@U\w+>)").unwrap();
    if let Some(caps) = re.captures(&event.text) {
        let name = caps.get(1).unwrap().as_str().to_string();
        return handle_say_insult(event, name).await;
    }

    let re = Regex::new(r"\binsult\s+me\b").unwrap();
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
    let message = match insults.read().map_err(|_| GenericError("somebody poisoned the insult cache!".to_string()))?.get_insult() {
        Some(insult) => format!("{} is {}", user_tag, insult),
        None => "Shut up.".to_string(),
    };

    send_message(&event.channel, &message).await
}

fn insert_word_to_cache(cache: &RwLock<InsultFactory>, pos: &PartOfSpeech, insult: String) -> LambdaResult<bool> {
    let mut insults = match cache.write() {
        Ok(i) => i,
        _ => return Err(Box::new(GenericError("somebody poisoned the insult cache!".to_string()))),
    };
    Ok(insults.insert_word(&pos, insult))
}

async fn handle_add_word(event: &MessageEvent, pos: PartOfSpeech, mut insult: String) -> LambdaResult<()> {
    let cache = insult_factory().await?;
    if !insert_word_to_cache(cache, &pos, insult.clone())? {
       return send_message(&event.channel, "I already have that word!").await;
    }
    let c = match pos {
        PartOfSpeech::Noun => 'n',
        PartOfSpeech::Adjective => 'a',
    };
    insult.push(c);
    insert_word_to_dynamo(insult).await?;
    send_message(&event.channel, "Added.").await
}
