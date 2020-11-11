use serde_derive::{Serialize, Deserialize};
use path_strings::*;


fn html_escape<T: AsRef<str>>(input: T) -> String {
    let mut string = String::with_capacity(input.as_ref().len());
    for c in input.as_ref().chars() {
        if c != '<' && c != '>' {
            string.push(c);
        } 
    }
    string
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

pub fn make_approved_comment(comment: &UncheckedComment, post_index: i64 ) -> ApprovedComments {
    let (t, reply_to) = extract_parent_post(comment.text.clone());
    
    ApprovedComments {
        comments: vec!(ApprovedComment {
            timestamp: comment.timestamp,
            author: comment.author.clone().map(html_escape),
            website: comment.website.clone().map(html_escape),
            text: markdown::to_html(&t),
            post_index: post_index,
            reply_to: reply_to,
        })
    }
}

pub async fn comment(web::Path(name): web::Path<String>, web::Form(form): web::Form<CommentForm>, data: web::Data<AppState>) -> impl Responder {
    match (form.author, form.text, form.website) {
        (author, text, website) if author.len() > 100 || text.len() > 10000 || website.len() > 500 =>
            error("Too long comment or name.", data).await,
        (_, text, _) if text.is_empty() => 
            error("Tried to submit empty comment.", data).await,
        (author, text, website) => {
            let time = std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap().as_secs();
            let comment = UncheckedComments { comments: vec!(UncheckedComment {
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

#[derive(Template)]
#[template(path = "comment_approvals.html", escape = "none")]
struct CommentApprovalsTemplate<'a> {
    layout: LayoutTemplate<'a>,
    comments: Vec<(String, &'a UncheckedComment)>,
    author_name_fn: fn (&Option<String>) -> String,
}

pub async fn comment_approval(req: web::HttpRequest, data: web::Data<AppState>) -> web::HttpResponse {
    if !auth_check(&req, &data.admin_password) {
        return unauthorized();
    }
    let comments: UncheckedComments = read_toml_default(&unverified_comments_path());
    let tmpl = CommentApprovalsTemplate {
        layout: layout_template(&data),
        comments: comments.comments.iter().map(|c| (markdown::to_html(&c.text), c)).collect(),
        author_name_fn: author_name_fn,
    };
    actix_web::HttpResponse::Ok().body(tmpl.render().unwrap())
}

pub async fn comment_approval_post(req: web::HttpRequest, body: String, data: web::Data<AppState>) -> web::HttpResponse {
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
