use std::rc::Rc;

use slint::ComponentHandle;

slint::include_modules!();

#[derive(Debug, serde::Deserialize)]
struct NotificationItem {
    id: String,
    unread: bool,
    reason: String,
    subject: SubjectItem,
    repository: RepositoryItem,
    url: String,
}

#[derive(Debug, serde::Deserialize)]
struct RepositoryItem {
    name: String,
    full_name: String,
    url: String,
    html_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct SubjectItem {
    title: String,
    url: Option<String>,
    latest_comment_url: Option<String>,
    #[serde(rename = "type")]
    type_: String,
}

static FETCH_ALL_NOTIFICATIONS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn main() {
    let home = home::home_dir();
    let hosts_path = home.unwrap().join(".config/gh/hosts.yml");
    if !hosts_path.exists() {
        println!("gh cli config not found");
    } else {
        let config = std::fs::read_to_string(hosts_path).unwrap();
        for line in config.lines() {
            if line.contains("oauth_token") {
                let token = line.split(": ").collect::<Vec<&str>>()[1];
                std::env::set_var("GITHUB_TOKEN", token);
                break;
            }
        }
    }

    let main_window = MainWindow::new().unwrap();
    std::thread::spawn({
        let main_window_weak = main_window.as_weak();
        move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(worker_fetch(main_window_weak))
        }
    });

    main_window.on_open_link({
        move |url| {
            println!("open link: {}", url);
            let u = String::from(url);
            open::that(u).unwrap();
        }
    });

    main_window.on_mark_read({
        move |id| {
            let id = String::from(id);
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(mark_thread_read(id))
        }
    });

    main_window.on_mark_done({
        move |id| {
            let id = String::from(id);
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(mark_thread_done(id))
        }
    });

    main_window.on_apply_filter({
        let mw = main_window.as_weak();
        move || {
            let unread_only = mw.unwrap().get_unread_only();
            println!("apply filter: {}", unread_only);
            FETCH_ALL_NOTIFICATIONS.store(!unread_only, std::sync::atomic::Ordering::Relaxed);

            std::thread::spawn({
                let mw = mw.clone();
                move || {
                    tokio::runtime::Runtime::new()
                        .unwrap()
                        .block_on(update_model(
                            mw.clone(),
                            &std::env::var("GITHUB_TOKEN").unwrap(),
                        ))
                }
            });
        }
    });

    main_window.run().unwrap();
}

async fn mark_thread_read(id: String) {
    println!("mark read: {}", id);
    let url = format!("https://api.github.com/notifications/threads/{}", id);
    let token = std::env::var("GITHUB_TOKEN").unwrap();

    let client = reqwest::Client::new();
    let response = client
        .patch(url)
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", format!("Bearer {}", token))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "ghn")
        .send()
        .await
        .unwrap();

    if response.status().is_success() {
        println!("mark read success");
    } else {
        println!("mark read failed");
    }

    let r = response.text().await.unwrap();
    println!("{}", r);
}

async fn mark_thread_done(id: String) {
    println!("mark done: {}", id);
    let url = format!("https://api.github.com/notifications/threads/{}", id);
    let token = std::env::var("GITHUB_TOKEN").unwrap();

    let client = reqwest::Client::new();
    let response = client
        .delete(url)
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", format!("Bearer {}", token))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "ghn")
        .send()
        .await
        .unwrap();

    if response.status().is_success() {
        println!("mark done success");
    } else {
        println!("mark done failed");
    }

    let r = response.text().await.unwrap();
    println!("{}", r);
}

async fn fetch_notifications(token: &str, all: bool) -> Vec<Notification> {
    let url = "https://api.github.com/notifications";
    let client = reqwest::Client::new();
    let query = vec![("all", all.to_string())];
    let response = client
        .get(url)
        .query(&query)
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", format!("Bearer {}", token))
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "ghn")
        .send()
        .await
        .unwrap();

    let r = response.text().await.unwrap();
    println!("{}", r);
    let notifications: Vec<NotificationItem> = serde_json::from_str(&r).unwrap();

    notifications
        .iter()
        .map(|item| Notification {
            id: item.id.clone().into(),
            unread: item.unread,
            reason: item.reason.clone().into(),
            url: item.url.clone().into(),
            subject: Subject {
                title: item.subject.title.clone().into(),
                url: item.subject.url.clone().unwrap_or_default().into(),
                latest_comment_url: item
                    .subject
                    .latest_comment_url
                    .clone()
                    .unwrap_or("".into())
                    .into(),
                type_: item.subject.type_.clone().into(),
            },
            repository: Repository {
                name: item.repository.name.clone().into(),
                full_name: item.repository.full_name.clone().into(),
                url: item.repository.url.clone().into(),
                html_url: item.repository.html_url.clone().into(),
            },
        })
        .collect::<Vec<Notification>>()
}

async fn worker_fetch(mw: slint::Weak<MainWindow>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    let token = std::env::var("GITHUB_TOKEN").unwrap();

    loop {
        interval.tick().await;

        update_model(mw.clone(), &token).await;
    }
}

async fn update_model(mw: slint::Weak<MainWindow>, token: &str) {
    let notifications = fetch_notifications(
        token,
        FETCH_ALL_NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed),
    )
    .await;
    mw.clone()
        .upgrade_in_event_loop(move |h| {
            let model = Rc::new(slint::VecModel::<Notification>::from(notifications));
            h.set_notifications_model(model.into());
        })
        .unwrap();
}
