use anyhow;
use dotenv::dotenv;
use http_req::request;
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use schedule_flows::schedule_cron_job;
use serde::Deserialize;
use slack_flows::send_message_to_channel;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use web_scraper_flows::get_page_text;

#[no_mangle]
pub fn run() {
    dotenv().ok();
    let keyword = std::env::var("KEYWORD").unwrap_or("ChatGPT".to_string());
    schedule_cron_job(String::from("14 * * * *"), keyword, callback);
}

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
async fn callback(keyword: Vec<u8>) {
    let query = String::from_utf8_lossy(&keyword);
    let now = SystemTime::now();
    let dura = now.duration_since(UNIX_EPOCH).unwrap().as_secs() - 3600;
    let url = format!("https://hn.algolia.com/api/v1/search_by_date?tags=story&query={query}&numericFilters=created_at_i>{dura}");

    let mut writer = Vec::new();
    if let Ok(_) = request::get(url, &mut writer) {
        if let Ok(search) = serde_json::from_slice::<Search>(&writer) {
            for hit in search.hits {
                let _ = send_message_wrapper(hit).await;
            }
        }
    }
}

#[derive(Deserialize)]
pub struct Search {
    pub hits: Vec<Hit>,
}

#[derive(Deserialize)]
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
        .split_whitespace()
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

    let question = format!("请用中文进行总结，字数最好在100个字内: {news_body}");

    match openai.chat_completion(&chat_id, &question, &co).await {
        Ok(r) => Ok(r.choice),
        Err(_e) => Err(anyhow::Error::msg(_e.to_string())),
    }
}

pub async fn send_message_wrapper(hit: Hit) -> anyhow::Result<()> {
    let workspace = env::var("slack_workspace").unwrap_or("secondstate".to_string());
    let channel = env::var("slack_channel").unwrap_or("test-flow".to_string());

    let title = &hit.title;
    let author = &hit.author;
    let post = format!("https://news.ycombinator.com/item?id={}", &hit.object_id);
    let mut source = "".to_string();

    let _text = match &hit.url {
        Some(u) => {
            source = format!("(<{u}|source>)");
            get_page_text(u)
                .await
                .unwrap_or("failed to scrape text with hit url".to_string())
        }
        None => {
            get_page_text(&post)
                .await
                .unwrap_or("failed to scrape text with post url".to_string())
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
