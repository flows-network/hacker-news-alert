
use schedule_flows::schedule_cron_job;
use chrono::{Duration, Utc};
use dotenv::dotenv;
use http_req::{request, request::Method, request::Request, uri::Uri};
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use slack_flows::{listen_to_channel, send_message_to_channel};
use std::env;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tiktoken_rs::cl100k_base;
// use web_scraper_flows::get_page_text;

#[no_mangle]
pub fn run() {
        dotenv().ok();
    let keyword = std::env::var("KEYWORD").unwrap_or("chatGPT".to_string());
    schedule_cron_job(String::from("25 * * * *"), keyword, callback);
}

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
async fn callback(keyword: Vec<u8>) {
    let workspace = env::var("slack_workspace").unwrap_or("secondstate".to_string());
    let channel = env::var("slack_channel").unwrap_or("github-status".to_string());

    let query = String::from_utf8_lossy(&keyword);
    let now = SystemTime::now();
    let dura = now.duration_since(UNIX_EPOCH).unwrap().as_secs() - 3600;
    let url = format!("https://hn.algolia.com/api/v1/search_by_date?tags=story&query={query}&numericFilters=created_at_i>{dura}");

    let mut writer = Vec::new();
    if let Ok(_) = request::get(url, &mut writer) {
        if let Ok(search) = serde_json::from_slice::<Search>(&writer) {
            for hit in search.hits {
                let title = &hit.title;
                let url = &hit.url;
                let object_id = &hit.object_id;
                let author = &hit.author;

                let post = format!("https://news.ycombinator.com/item?id={object_id}");

                match url {
                    Some(u) => {
                        let source = format!("(<{u}|source>)");
                        if let Some(text) = obtain_summary_by_post(u).await {
                            // if let Some(summary) = get_summary_truncated(text).await {
                            //     send_message_to_channel("ik8", "ch_err", summary.clone()).await;

                                let msg = format!(
                                    "- *{title}*\n<{post} | post>{source} by {author}\n{text}"
                                );
                                send_message_to_channel(&workspace, &channel, msg).await;
                            // }
                        }
                    }
                    None => {
                        if let Some(text) = obtain_text_by_post(&post).await {
                            if let Some(summary) = get_summary_truncated(text).await {
                                let msg =
                                    format!("- *{title}*\n<{post} | post> by {author}\n{summary}");
                                send_message_to_channel(&workspace, &channel, msg).await;
                            }
                        }
                    }
                };
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

pub async fn obtain_text_by_post(inp: &str) -> Option<String> {
    let server_addr = "43.135.155.64:3000".parse::<SocketAddr>().unwrap();
    let url = format!("http://{}/api", server_addr);
    let uri = Uri::try_from(url.as_ref()).unwrap();
    let body = json!({ "url": inp }).to_string();

    let mut writer = Vec::<u8>::new();
    if let Ok(_res) = Request::new(&uri)
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .header("Content-Length", &body.len())
        .body(body.as_bytes())
        .send(&mut writer)
    {
        let text_load = String::from_utf8_lossy(&writer);
        return Some(text_load.to_string());
    }
    None
}

pub async fn obtain_summary_by_post(inp: &str) -> Option<String> {
    let server_addr = "43.135.155.64:4000".parse::<SocketAddr>().unwrap();
    let url = format!("http://{}/api", server_addr);
    let uri = Uri::try_from(url.as_ref()).unwrap();
    let body = json!({ "url": inp }).to_string();

    let mut writer = Vec::<u8>::new();
    if let Ok(_res) = Request::new(&uri)
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .header("Content-Length", &body.len())
        .body(body.as_bytes())
        .send(&mut writer)
    {
        let text_load = String::from_utf8_lossy(&writer);
        return Some(text_load.to_string());
    }
    None
}

async fn get_summary_truncated(inp: String) -> Option<String> {
    let mut openai = OpenAIFlows::new();
    openai.set_retry_times(3);

    let news_body = inp
        .split_ascii_whitespace()
        .take(3000)
        .collect::<Vec<&str>>()
        .join(" ");

    let chat_id = format!("news summary N");
    let system = &format!("You're an editor AI.");

    let co = ChatOptions {
        model: ChatModel::GPT35Turbo,
        restart: true,
        system_prompt: Some(system),
    };

    let question = format!("Summarize this: {news_body}.");
    // let question = format!("Given the news body text: {news_body}, which may include some irrelevant information, identify the key arguments and the article's conclusion. From these important elements, construct a succinct summary that encapsulates its news value, disregarding any unnecessary details.");

    match openai.chat_completion(&chat_id, &question, &co).await {
        Ok(r) => Some(r.choice),
        Err(_e) => None,
    }
}


async fn get_text_private(inp: &str) -> Option<String> {
    let url = format!("http://43.135.155.64:3000/?url={inp}");
    let mut writer = Vec::<u8>::new();
    if let Ok(_) = request::get(url, &mut writer) {
        Some(String::from_utf8(writer).unwrap_or("failed to parse text from private web scraper".to_string()))
    } else {
        None
    }
}
async fn get_summary(inp: String) -> String {
    let mut openai = OpenAIFlows::new();
    openai.set_retry_times(3);

    let bpe = cl100k_base().unwrap();

    let feed_tokens_map = bpe.encode_ordinary(&inp);

    let chat_id = format!("news summary N");
    let system = &format!("As a news reporter AI,");

    let co = ChatOptions {
        model: ChatModel::GPT35Turbo,
        restart: true,
        system_prompt: Some(system),
    };

    let total_tokens_count = feed_tokens_map.len();
    let mut _summary = "".to_string();

    if total_tokens_count > 2800 {
        let mut token_vec = feed_tokens_map;
        let mut map_out = "".to_string();

        while !token_vec.is_empty() {
            let drain_to = std::cmp::min(token_vec.len(), 2800);
            let token_chunk = token_vec.drain(0..drain_to).collect::<Vec<_>>();

            let text_chunk = bpe.decode(token_chunk).unwrap();

            send_message_to_channel("ik8", "ch_in", text_chunk.clone()).await;

            let map_question = format!("This is a segment of the text from the news page: '{text_chunk}'. It may contain irrelevant information due to ads or the publisher's intention to leverage this news' public attention to promote other agenda. Extract and summarize the key information that may be connected to the news from this segment.");

            match openai.chat_completion(&chat_id, &map_question, &co).await {
                Ok(r) => {
                    send_message_to_channel("ik8", "ch_out", r.choice.clone()).await;
                    map_out.push_str(&r.choice);
                }
                Err(_e) => {}
            }
        }

        let reduce_question = format!("Given the key information extracted from the news' body text: {map_out}, focus on the core arguments and the conclusions drawn in the article. Create a brief and meaningful summary that captures its relevance and news-worthiness.");

        match openai
            .chat_completion(&chat_id, &reduce_question, &co)
            .await
        {
            Ok(r) => {
                _summary = r.choice;
            }
            Err(_e) => {}
        }
    } else {
        let news_body = bpe.decode(feed_tokens_map).unwrap();

        let question = format!("Given the news body text: {news_body}, which may include some irrelevant information, identify the key arguments and the article's conclusion. From these important elements, construct a succinct summary that encapsulates its news value, disregarding any unnecessary details.");

        match openai.chat_completion(&chat_id, &question, &co).await {
            Ok(r) => {
                _summary = r.choice;
            }
            Err(_e) => {}
        }
    }

    _summary
}
