use anyhow;
use dotenv::dotenv;
use http_req::request;
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use schedule_flows::schedule_cron_job;
use serde::{Deserialize, Serialize};
use slack_flows::send_message_to_channel;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fmt::format};
use web_scraper_flows::get_page_text;

#[no_mangle]
pub fn run() {
    dotenv().ok();
    let keyword = std::env::var("KEYWORD").unwrap_or("chatGPT".to_string());
    schedule_cron_job(String::from("57 * * * *"), keyword, callback);
}

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
async fn callback(keyword: Vec<u8>) {
    let query = String::from_utf8_lossy(&keyword);
    let now = SystemTime::now();
    let dura = now.duration_since(UNIX_EPOCH).unwrap().as_secs() - 30000;
    let url = format!("https://hn.algolia.com/api/v1/search_by_date?tags=story&query={query}&numericFilters=created_at_i>{dura}");

    let mut writer = Vec::new();
    if let Ok(_) = request::get(url, &mut writer) {
        if let Ok(search) = serde_json::from_slice::<Search>(&writer) {
            for hit in search.hits {
                send_message_wrapper(hit).await;
            }
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Search {
    pub hits: Vec<Hit>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Hit {
    pub title: String,
    pub url: Option<String>,
    #[serde(rename = "objectID")]
    pub object_id: String,
    pub author: String,
    pub created_at_i: i64,
}

async fn get_summary_truncated(inp: &str) -> anyhow::Result<String> {
    let mut openai = OpenAIFlows::new();
    openai.set_retry_times(3);

    let news_body = inp
        .split_ascii_whitespace()
        .take(10000)
        .collect::<Vec<&str>>()
        .join(" ");

    let chat_id = format!("summary#99");
    let system = &format!("You're an AI assistant.");

    let co = ChatOptions {
        model: ChatModel::GPT35Turbo16K,
        restart: true,
        system_prompt: Some(system),
    };

    let question = format!("summarize this within 100 words: {news_body}");

    match openai.chat_completion(&chat_id, &question, &co).await {
        Ok(r) => Ok(r.choice),
        Err(_e) => Err(anyhow::Error::msg(_e.to_string())),
    }
}

pub async fn send_message_wrapper(hit: Hit) -> anyhow::Result<()> {
    let workspace = env::var("slack_workspace").unwrap_or("secondstate".to_string());
    let channel = env::var("slack_channel").unwrap_or("github-status".to_string());

    let title = &hit.title;
    let url = &hit.url;
    let object_id = &hit.object_id;
    let author = &hit.author;
    let post = format!("https://news.ycombinator.com/item?id={object_id}");
    let mut _text = "".to_string();
    let mut source = "".to_string();

    match url {
        Some(u) => {
            source = format!("(<{u}|source>)");
            _text = get_page_text(u)
                .await
                .unwrap_or("failed to scrape text with hit url".to_string());

            // let out = format!("arm with source, summary: {summary}, text: {text}");
            // send_message_to_channel("ik8", "ch_err", out).await;
        }
        None => {
            _text = get_page_text(&post)
                .await
                .unwrap_or("failed to scrape text with post url".to_string());

            // let out = format!("arm no source, summary: {summary}, text: {text}");
            // send_message_to_channel("ik8", "ch_out", out).await;
        }
    };
    
    let summary = if _text.split_whitespace().count() > 100 {
        get_summary_truncated(&_text).await?
    } else {
        _text
    };

    let msg = format!("- *{title}*\n<{post} | post>{source} by {author}\n{summary}");

    send_message_to_channel(&workspace, &channel, msg).await;
    Ok(())
}
