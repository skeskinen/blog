use actix_web::{web, Responder};
use actix_service::Service;
use askama::Template;
use serde_derive::{Serialize, Deserialize};
use std::cell::{RefCell, Cell};
use std::num::Wrapping;
use std::collections::HashMap;
use std::iter::FromIterator;
use std::io::Write;
use std::fs::OpenOptions;
use std::sync::{Arc, RwLock, Mutex};

#[derive(Default, Clone, Copy)]
struct RandomGenerator {
    state: Wrapping<u32>,
}

fn init_rng() -> RandomGenerator {
    RandomGenerator {state: Wrapping(0)}
}

fn get_random(generator: &Cell<RandomGenerator>) -> u32 {
    let s = generator.get().state * Wrapping(2147001325) + Wrapping(715136305);
    generator.replace(RandomGenerator{state: s});
    return 0x31415926 ^ ((s.0 >> 16) + (s.0 << 16));
}

fn author_name(author: &Option<String>) -> String {
    author.clone().unwrap_or("Anon".to_string())
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct UncheckedComment {
    timestamp: u64,
    author: Option<String>,
    website: Option<String>,
    text: String,
    article: String,
}

#[derive(Default, Serialize, Deserialize)]
struct UncheckedComments {
    comments: Vec<UncheckedComment>,
}

#[derive(Default, Serialize, Deserialize)]
struct ApprovedComment {
    timestamp: u64,
    author: Option<String>,
    website: Option<String>,
    text: String,
    post_index: i64,
    reply_to: Option<i64>,
}

#[derive(Default, Serialize, Deserialize)]
struct ApprovedComments {
    comments: Vec<ApprovedComment>,
}

#[derive(Default, Serialize, Deserialize)]
struct DisplayComment {
    date: String,
    author: String,
    website: Option<String>,
    text: String,
    post_index: i64,
    reply_to: Option<i64>,
    replies: Vec<i64>
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct RecentComments {
    recent_comments: Vec<String>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct CompactedLog {
    entries: HashMap<String, usize>,
}

struct AppState {
    rng: Cell<RandomGenerator>,
    quote_data: Quotes,
    meta: Meta,
    log_file_draft_lock: Arc<Mutex<()>>,
    log_output: RefCell<(std::fs::File, chrono::Date<chrono::Utc>)>,
    unchecked_comments_file_lock: Arc<Mutex<()>>,
    recent_comments: Arc<RwLock<TomlFile<RecentComments>>>,
    admin_password: String,
}

struct LayoutTemplate<'a> {
    quote_text: &'a str,
    quote_author: &'a str,
    tags: &'a Vec<Tag>,
    recent_comments: Vec<Article>,
    recent_articles: Vec<Article>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    layout: LayoutTemplate<'a>,
    articles: &'a Vec<Article>,
}

#[derive(Template)]
#[template(path = "article.html", escape = "none")]
struct ArticleTemplate<'a> {
    layout: LayoutTemplate<'a>,
    article: &'a Article,
    content: &'a str,
    comments: Vec<DisplayComment>,
}

#[derive(Template)]
#[template(path = "about.html")]
struct AboutTemplate<'a> {
    layout: LayoutTemplate<'a>,
}

#[derive(Template)]
#[template(path = "archive.html")]
struct ArchiveTemplate<'a> {
    layout: LayoutTemplate<'a>,
}

#[derive(Template)]
#[template(path = "tag.html")]
struct TagTemplate<'a> {
    layout: LayoutTemplate<'a>,
    tag: &'a Tag,
    articles: Vec<Article>,
}

#[derive(Template)]
#[template(path = "404.html")]
struct P404Template<'a> {
    layout: LayoutTemplate<'a>,
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate<'a> {
    layout: LayoutTemplate<'a>,
    error: &'a str,
}

#[derive(Template)]
#[template(path = "comment_approvals.html", escape = "none")]
struct CommentApprovalsTemplate<'a> {
    layout: LayoutTemplate<'a>,
    comments: Vec<(String, &'a UncheckedComment)>,
    author_name: fn (&Option<String>) -> String,
}

#[derive(Template)]
#[template(path = "stats.html")]
struct StatsTemplate<'a> {
    layout: LayoutTemplate<'a>,
    stats: &'a Vec<(String, usize)>,
}

#[derive(Clone, Deserialize)]
struct Quote {
    text: String,
    author: String,
}

#[derive(Clone, Deserialize)]
struct Quotes {
    quotes: Vec<Quote>,
}

#[derive(Clone, Deserialize)]
struct Project {
    name: String,
    title: String,
    order: i32,
}

#[derive(Clone, Deserialize)]
struct Article {
    name: String,
    description: String,
    title: String,
    date: toml::value::Datetime,
    tags: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct MetaFile {
    tags: Vec<String>,
    projects: Vec<Project>,
    articles: Vec<Article>,
}

#[derive(Clone, Deserialize)]
struct Tag {
    name: String,
    count: i32,
    articles: Vec<String>,
}

#[derive(Clone, Deserialize)]
struct Meta {
    tags: Vec<Tag>,
    projects_map: HashMap<String, (Project, String)>,
    articles_map: HashMap<String, (Article, String)>,
    recent_articles: Vec<Article>,
}

fn layout_template(data: &web::Data<AppState>) -> LayoutTemplate {
    let rng = get_random(&data.rng) as usize;
    let quotes_length = data.quote_data.quotes.len();
    let quote_index = rng % quotes_length;
    LayoutTemplate {
        quote_text: &data.quote_data.quotes[quote_index].text,
        quote_author: &data.quote_data.quotes[quote_index].author,
        tags: &data.meta.tags,
        recent_comments: data.recent_comments.read().unwrap().toml.recent_comments.iter().map(|rc| data.meta.articles_map.get(rc).unwrap().0.clone()).collect(),
        recent_articles: data.meta.recent_articles.iter().take(6).map(|i| i.clone()).collect(),
    }
}

async fn p404(data: web::Data<AppState>) -> actix_web::HttpResponse {
    let tmpl = P404Template {
        layout: layout_template(&data),
    };
    actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
}

async fn index(data: web::Data<AppState>) -> impl Responder {
    let tmpl = IndexTemplate {
        layout: layout_template(&data),
        articles: &data.meta.recent_articles,
    };
    actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
}

async fn article(web::Path(name): web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    match data.meta.articles_map.get(&name) {
        Some((a, md)) => {
            let comments: ApprovedComments = read_toml_default(&comments_path(&name));
            let mut display_comments: Vec<DisplayComment> = Vec::with_capacity(comments.comments.len());
            for c in comments.comments {
                match c.reply_to {
                    None => (),
                    Some(parent) =>
                        match display_comments.iter_mut().find(|dc| dc.post_index == parent) {
                            None => (),
                            Some(parent_post) => parent_post.replies.push(c.post_index),
                        }
                }
                display_comments.push(DisplayComment {
                    author: author_name(&c.author),
                    website: c.website,
                    date: timestamp_to_datestring(&c.timestamp),
                    reply_to: c.reply_to,
                    post_index: c.post_index,
                    text: c.text,
                    replies: Vec::new(),
                });
            }
            let tmpl = ArticleTemplate {
                layout: layout_template(&data),
                article: &a,
                content: &md, 
                comments: display_comments,
            };
            actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
        },
        None => p404(data).await
    }
}

async fn tag(web::Path(name): web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    match data.meta.tags.iter().find(|&x| x.name == name) {
        Some(tag_meta) => {
            let tmpl = TagTemplate {
                layout: layout_template(&data),
                tag: tag_meta,
                articles: tag_meta.articles.iter().map(|t| data.meta.articles_map[t].0.clone()).collect(),
            };
            actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
        },
        None => p404(data).await
    }
}

async fn about(data: web::Data<AppState>) -> impl Responder {
    let tmpl = AboutTemplate {
        layout: layout_template(&data),
    };
    actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
}

async fn archive(data: web::Data<AppState>) -> impl Responder {
    let tmpl = ArchiveTemplate {
        layout: layout_template(&data),
    };
    actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
}

async fn error(error: &str, data: web::Data<AppState>) -> web::HttpResponse {
    let tmpl = ErrorTemplate {
        layout: layout_template(&data),
        error: error,
    };
    actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
}

#[derive(Deserialize)]
struct CommentForm {
    author: String,
    text: String,
    website: String,
}

fn timestamp_to_datestring(timestamp: &u64) -> String {
    chrono::NaiveDateTime::from_timestamp(timestamp.clone() as i64, 0).format("%Y-%m-%d").to_string()
}

fn extract_parent_post(text: String) -> (String, Option<i64>) {
    let t = text.trim();
    if t.starts_with("@") {
        let chrs: &[char] = &[' ', '\n', '\t', '\r'];
        let num_str = (&t[1..]).split(chrs).next().unwrap_or_default();
        match str::parse::<i64>(&num_str) {
            Ok(d) => {
                (t[1..].trim_start_matches(|c| (c >= '0' && c <= '9') || c == ' ' || c == '\t' || c == '\r' || c == '\n').to_string(), Some(d))
            },
            Err(_) => 
                (t.to_string(), None)
        }
    } else {
        (t.to_string(), None)
    }
}

fn append_to_file(path: &str, content: &str) {
    let mut file = OpenOptions::new().append(true).create(true).open(path).unwrap();
    write!(file, "{}", content).unwrap();
}

async fn comment(web::Path(name): web::Path<String>, web::Form(form): web::Form<CommentForm>, data: web::Data<AppState>) -> impl Responder {
    match (form.author, form.text, form.website) {
        (author, text, website) if author.len() > 100 || text.len() > 10000 || website.len() > 500 =>
            error("Too long comment or name.", data).await,
        (_, text, _) if text.is_empty() => 
            error("Tried to submit empty comment.", data).await,
        (author, text, website) => {
            let time = std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs();
            let comment = UncheckedComments{ comments: vec!(UncheckedComment {
                timestamp: time,
                author: if author.is_empty() { None } else { Some(author) },
                website: if website.is_empty() { None } else { Some(website) },
                article: name.clone(),
                text: text,
            })};
            {
                let _lock_guard = data.unchecked_comments_file_lock.lock().unwrap();
                append_to_file(&unverified_comments_path(), &toml::to_string(&comment).unwrap());
            }
            actix_web::HttpResponse::Found()
                .header(actix_web::http::header::LOCATION, format!("/a/{}", name)).finish()
        }
    }
}

async fn comment_approval(req: web::HttpRequest, data: web::Data<AppState>) -> web::HttpResponse {
    if !auth_check(&req, &data.admin_password) {
        return unauthorized();
    }
    let comments: UncheckedComments = read_toml_default(&unverified_comments_path());
    let tmpl = CommentApprovalsTemplate {
        layout: layout_template(&data),
        comments: comments.comments.iter().map(|c| (markdown::to_html(&c.text), c)).collect(),
        author_name: author_name,
    };
    actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
}

fn make_approved_comment(comment: &UncheckedComment, post_index: i64 ) -> ApprovedComments {
    let (t, reply_to) = extract_parent_post(comment.text.clone());
    ApprovedComments {
        comments: vec!(ApprovedComment {
            timestamp: comment.timestamp,
            author: comment.author.clone(),
            website: comment.website.clone(),
            text: markdown::to_html(&t),
            post_index: post_index,
            reply_to: reply_to,
        })
    }
}

fn fetch_incr_count(value: &mut toml::Value, key: &str) -> i64 {
    match value {
        toml::Value::Table(t) => {
            match t.get_mut(key) {
                Some(v) => {
                    if let toml::Value::Integer(res) = v {
                        *res += 1;
                        *res
                    } else {
                        panic!("count was not integer");
                    }
                },
                None => {
                    t.insert(key.to_string(), toml::Value::Integer(0));
                    0
                }
            }
        },
        _ => panic!("unexpected"),
    }
}

async fn comment_approval_post(req: web::HttpRequest, body: String, data: web::Data<AppState>) -> web::HttpResponse {
    if !auth_check(&req, &data.admin_password) {
        return unauthorized();
    }
    let mut approved_comments: Vec<(String, ApprovedComments)> = Vec::new();
    {
        let _lock_guard = data.unchecked_comments_file_lock.lock().unwrap();
        let comment_list = read_toml::<UncheckedComments>(&unverified_comments_path()).comments;
        let mut new_unchecked_data = UncheckedComments { comments: Vec::new() };
        let mut comment_counts: TomlFile<toml::Value> = TomlFile::read(&comment_counts_path());
    
        let mut count = 0;
        for item in body.split('&') {
            let c = &comment_list[count];
            match item.split('=').skip(1).next() {
                Some("ignore") => new_unchecked_data.comments.push(c.clone()),
                Some("approve") => approved_comments.push((c.article.clone(), make_approved_comment(&c, fetch_incr_count(&mut comment_counts.toml, &c.article)))),
                Some("delete") => (),
                _ => unreachable!(),
            }
            count += 1;
        }
        for i in count .. comment_list.len() {
            new_unchecked_data.comments.push(comment_list[i].clone());
        }
        if new_unchecked_data.comments.is_empty() {
            std::fs::write(unverified_comments_path(), "").unwrap();
        } else {
            std::fs::write(unverified_comments_path(), toml::to_string(&new_unchecked_data).unwrap()).unwrap();
        }

        comment_counts.write();
    }
    std::fs::create_dir_all(&comments_dir()).unwrap();

    let mut recent_comments = data.recent_comments.read().unwrap().toml.recent_comments.clone();

    for (key, val) in approved_comments {
        append_to_file(&comments_path(&key), &toml::to_string(&val).unwrap());
        if let Some(i) = recent_comments.iter().position(|rc| rc == &key) {
            recent_comments.remove(i);
        }
        if recent_comments.len() > 6 {
            recent_comments.pop();
        }
        recent_comments.insert(0, key);
    }
    {
        let mut w = data.recent_comments.write().unwrap();
        w.toml = RecentComments { recent_comments: recent_comments };
        w.write();
    }

    comment_approval(req, data).await
}

fn get_compacted_log(mut path: std::path::PathBuf) -> CompactedLog {
    path.push("compacted.toml");
    if path.exists() {
        read_toml(&path.to_string_lossy())
    } else {
        path.pop();
        let mut cl = CompactedLog { entries: HashMap::new() };
        for file in path.read_dir().unwrap() {
            let log_contents = std::fs::read_to_string(file.unwrap().path()).unwrap();
            for line in log_contents.split('\n') {
                if !line.is_empty() {
                    cl.entries.entry(line.to_string())
                        .and_modify(|v| *v += 1)
                        .or_insert(1);
                }
            }
        }
        path.push("compacted.toml");
        std::fs::write(path, toml::to_string(&cl).unwrap()).unwrap();
        cl
    }
}

fn sum_compacted_log(a: CompactedLog, mut b: CompactedLog) -> CompactedLog {
    for (k, v) in a.entries {
        b.entries.entry(k)
            .and_modify(|v2| *v2 += v)
            .or_insert(v);
    }
    b
}

fn base64_char_value(c: char) -> u8 {
    match c {
        'A'..='Z' => c as u8 - 'A' as u8,
        'a'..='z' => c as u8 - 'a' as u8 + 26,
        '0'..='9' => c as u8 - '0' as u8 + 52,
        '+' => 62,
        '/' => 63,
        '=' => 0,
        _ => panic!("invalid base64 input"),
    }
}

fn base64_decode(input: &str) -> String {
    let mut res: Vec<u8> = Vec::new();
    let mut filled_bits = 0;
    for c in input.chars() {
        if c == '=' {
            res.pop();
            break;
        }
        let v = base64_char_value(c);
        match filled_bits {
            0 => {
                res.push(base64_char_value(c) << 2);
                filled_bits = 6;
            }
            2 => {
                *res.last_mut().unwrap() |= v;
                filled_bits = 0;
            },
            4 | 6 =>  {
                *res.last_mut().unwrap() |= v >> (filled_bits - 2);
                filled_bits -= 2;
                res.push(v << (8 - filled_bits));
            },
            _ => panic!("bad base64 decoder"),
        }
    }
    unsafe { String::from_utf8_unchecked(res) }
}

fn auth_check(req: &web::HttpRequest, password: &str) -> bool {
    if let Some(auth) = req.headers().get("Authorization") {
        let mut s = auth.to_str().unwrap_or_default().split(' ');
        match (s.next(), s.next(), s.next()) {
            (_, _, Some(_)) => { println!("Extra auth string"); false },
            (Some("Basic"), Some(sign), _) => {
                &base64_decode(sign) == password
            },
            _ => { println!("Not basic auth"); false },
        }
    } else {
        false
    }
}

fn unauthorized() -> actix_web::HttpResponse {
    actix_web::HttpResponse::Unauthorized()
        .header("WWW-Authenticate", "Basic realm=\"Lesser Scholar\", charset=\"UTF-8\"")
        .finish()
}

async fn stats(req: web::HttpRequest, data: web::Data<AppState>) -> actix_web::HttpResponse {
    if !auth_check(&req, &data.admin_password) {
        return unauthorized();
    }
    let mut compacted = CompactedLog { entries: HashMap::new() };
    for d in std::path::Path::new(&logs_path()).read_dir().unwrap() {
        match d {
            Err(_) => (),
            Ok(dir) => if dir.file_name().to_string_lossy().to_string() != log_date_string(chrono::Utc::today()) {
                let cl = get_compacted_log(dir.path());
                compacted = sum_compacted_log(compacted, cl);
            }
        }
    }
    let mut stats: Vec<(String, usize)> = compacted.entries.into_iter().collect();
    stats.sort_by(|a, b| (b.1).cmp(&a.1));
    let tmpl = StatsTemplate {
        layout: layout_template(&data),
        stats: &stats,
    };
    actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
}


fn make_meta(meta_file: MetaFile) -> Meta {
    let articles = HashMap::from_iter(meta_file.articles.clone().into_iter().map(|a| {
        let file_path = format!("articles/{}.md", a.name);
        let md = markdown::file_to_html(std::path::Path::new(&file_path)).expect(&format!("Failed to open article {}", a.name));
        (a.name.clone(), (a, md))
    }));
    let projects = HashMap::from_iter(meta_file.projects.into_iter().map(|p| {
        let file_path = format!("projects/{}.md", p.name);
        let md = markdown::file_to_html(std::path::Path::new(&file_path)).expect(&format!("Failed to open project {}", p.name));
        (p.name.clone(), (p, md))
    }));
    let mut tags: Vec<Tag> = meta_file.tags.iter().map(|t| Tag {name: t.clone(), count: 0, articles: Vec::new()}).collect();
    for a in &meta_file.articles {
        for t in &a.tags {
            for tt in &mut tags {
                if &tt.name == t {
                    tt.articles.push(a.name.clone());
                    tt.count += 1;
                    break;
                }
            }
        }
    }
    tags.sort_by_key(|t| -t.count);

    Meta {
        articles_map: articles,
        projects_map: projects,
        tags: tags,
        recent_articles: meta_file.articles
    }
}

fn blog_data_dir() -> String {
    let path: std::path::PathBuf = [&std::env::var("HOME").unwrap(), "blog_data"].iter().collect();
    path.to_string_lossy().to_string()
}

fn unverified_comments_path() -> String {
    let path: std::path::PathBuf = [&blog_data_dir(), "unverified_comments.toml"].iter().collect();
    path.to_string_lossy().to_string()
}

fn comment_counts_path() -> String {
    let path: std::path::PathBuf = [&blog_data_dir(), "comment_counts.toml"].iter().collect();
    path.to_string_lossy().to_string()
}

fn recent_comments_path() -> String {
    let path: std::path::PathBuf = [&blog_data_dir(), "recent_comments.toml"].iter().collect();
    path.to_string_lossy().to_string()
}

fn comments_dir() -> String {
    let path: std::path::PathBuf = [&blog_data_dir(), "comments"].iter().collect();
    path.to_string_lossy().to_string()
}

fn comments_path(article: &str) -> String {
    let path: std::path::PathBuf = [&blog_data_dir(), "comments", &format!("{}.toml", article)].iter().collect();
    path.to_string_lossy().to_string()
}

fn logs_path() -> String {
    let path: std::path::PathBuf = [&blog_data_dir(), "logs"].iter().collect();
    path.to_string_lossy().to_string()
}

fn log_date_string(date: chrono::Date<chrono::Utc>) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn log_file(lock: Arc<Mutex<()>>) -> (std::fs::File, chrono::Date<chrono::Utc>) {
    let today = chrono::Utc::today();
    let mut path: std::path::PathBuf = [&blog_data_dir(), "logs", &log_date_string(today)].iter().collect();
    std::fs::create_dir_all(&path).expect("Failed to create log dir");
    let _lock_result = lock.lock();
    let file_count = std::path::Path::new(&path).read_dir().unwrap().count();
    path.push(format!("{}.log", file_count));
    (std::fs::File::create(path).unwrap(), today)
}

#[derive(Clone)]
struct TomlFile<T> {
    path: String,
    toml: T,
}

impl<T> TomlFile<T> where T: serde::de::DeserializeOwned {
    fn read(path: &str) -> TomlFile<T> {
        let data = std::fs::read_to_string(path).unwrap_or_default();
        TomlFile {
            path: path.to_string(),
            toml: toml::from_str(&data).unwrap(),
        }
    }
}

impl<T> TomlFile<T> where T: serde::de::DeserializeOwned + Default{
    fn read_default(path: &str) -> TomlFile<T> {
        let data = std::fs::read_to_string(path).unwrap_or_default();
        TomlFile {
            path: path.to_string(),
            toml: toml::from_str(&data).unwrap_or_default(),
        }
    }
}

impl<T> TomlFile<T> where T: serde::Serialize {
    fn write(&self) {
        std::fs::write(&self.path, &toml::to_string(&self.toml).unwrap()).unwrap();
    }
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &str) -> T {
    let data = std::fs::read_to_string(path).unwrap_or_default();
    toml::from_str(&data).unwrap()
}

fn read_toml_default<T: serde::de::DeserializeOwned + Default>(path: &str) -> T {
    let data = std::fs::read_to_string(path).unwrap_or_default();
    toml::from_str(&data).unwrap_or_default()
}

fn get_admin_password() -> String {
    if std::path::Path::new("admin_password.txt").exists() {
        std::fs::read_to_string("admin_password.txt").unwrap()
    } else {
        "admin:password".to_string()
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let quotes: Quotes = read_toml("src/quotes.toml");
    let meta = make_meta(read_toml("src/meta.toml"));
    
    let log_file_draft_lock = Arc::new(Mutex::new(()));
    let unchecked_comments_file_lock = Arc::new(Mutex::new(()));

    let recent_comments = Arc::new(RwLock::new(TomlFile::read_default(&recent_comments_path())));
    let password = get_admin_password();

    actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .data(AppState {
                rng: Cell::new(init_rng()),
                quote_data: quotes.clone(),
                meta: meta.clone(),
                log_file_draft_lock: log_file_draft_lock.clone(),
                log_output: RefCell::new(log_file(log_file_draft_lock.clone())),
                unchecked_comments_file_lock: unchecked_comments_file_lock.clone(),
                recent_comments: recent_comments.clone(),
                admin_password: password.clone(),
            })
            .wrap_fn(|req, srv| {
                let data: &actix_web::web::Data<AppState> = req.app_data().unwrap();
                let today = chrono::Utc::today();
                if today > data.log_output.borrow().1 {
                    data.log_output.replace(log_file(data.log_file_draft_lock.clone()));
                }
                write!(data.log_output.borrow_mut().0, "{} {}\n", req.method(), req.path()).unwrap_or(());

                srv.call(req)
            })
            .route("/", web::get().to(index))
            .route("/about", web::get().to(about))
            .route("/archive", web::get().to(archive))
            .route("/a/{name}", web::get().to(article))
            .route("/tag/{name}", web::get().to(tag))
            .route("/comment/{name}", web::post().to(comment))
            .route("/comment_approval", web::get().to(comment_approval))
            .route("/comment_approval", web::post().to(comment_approval_post))
            .route("/stats", web::get().to(stats))
            .service(actix_files::Files::new("/", "static"))
            .default_service(web::get().to(p404))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
