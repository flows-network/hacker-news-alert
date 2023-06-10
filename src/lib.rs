use dotenv::dotenv;
use http_req::request;
use openai_flows::{
    chat::{ChatModel, ChatOptions},
    OpenAIFlows,
};
use schedule_flows::schedule_cron_job;
use serde_derive::{Deserialize, Serialize};
use slack_flows::send_message_to_channel;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};
use tiktoken_rs::cl100k_base;
use tokio;
use web_scraper_flows::get_page_text;

#[no_mangle]
pub fn run() {
    let keyword = std::env::var("KEYWORD").unwrap();
    schedule_cron_job(String::from("47 * * * *"), keyword, callback);
}

#[no_mangle]
#[tokio::main(flavor = "current_thread")]
async fn callback(keyword: Vec<u8>) {
    dotenv().ok();

    let workspace = std::env::var("slack_workspace").unwrap();
    let channel = std::env::var("slack_channel").unwrap();

    let query = String::from_utf8(keyword).unwrap();
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
                let source = match url {
                    Some(u) => format!("(<{u}|source>)"),
                    None => String::new(),
                };
                let msg = format!("- *{title}*\n<{post} | post>{source} by {author}\n");
                send_message_to_channel(&workspace, &channel, msg);

                if let Ok(text) = get_page_text(url.clone().unwrap().as_ref()).await {
                    let msg =
                    format!("- *{title}*\n<{post} | post>{source} by {author}\n{text}");
                send_message_to_channel(&workspace, &channel, msg);

                     let summary = get_summary(text).await;
                        let msg =
                            format!("- *{title}*\n<{post} | post>{source} by {author}\n{summary}");
                        send_message_to_channel(&workspace, &channel, msg);
                }
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

async fn get_summary(inp: String) -> String {
    let mut openai = OpenAIFlows::new();
    openai.set_retry_times(3);

    let bpe = cl100k_base().unwrap();

    let feed_tokens_map = bpe.encode_ordinary(&inp);

    let chat_id = format!("news summary N");
    let system = &format!("You're a news reporter bot");

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

            let map_question = format!("This is segment of news '{text_chunk}'");

            match openai.chat_completion(&chat_id, &map_question, &co).await {
                Ok(r) => {
                    map_out.push_str(&r.choice);
                }
                Err(_e) => {}
            }
        }

        let reduce_question = format!("The key information you've extracted from the news' body text: {map_out}. Concentrate on the key arguments, and the conclusion the article is trying to make. From these elements, generate a concise summary that reflects its news-worthyness.");

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

        let question = format!("This is the news body text: {news_body}. Concentrate on the key arguments, and the conclusion the article is trying to make. From these elements, generate a concise summary that reflects its news-worthyness.");

        match openai.chat_completion(&chat_id, &question, &co).await {
            Ok(r) => {
                _summary = r.choice;
            }
            Err(_e) => {}
        }
    }

    _summary
}
