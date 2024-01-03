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
}

#[derive(Debug, serde::Deserialize)]
struct SubjectItem {
    title: String,
    url: Option<String>,
    latest_comment_url: Option<String>,
    #[serde(rename = "type")]
    type_: String,
}

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

    main_window.run().unwrap();
}

async fn worker_fetch(mw: slint::Weak<MainWindow>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    let token = std::env::var("GITHUB_TOKEN").unwrap();

    loop {
        interval.tick().await;

        let url = "https://api.github.com/notifications";
        let client = reqwest::Client::new();
        let response = client
            .get(url)
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

        let notifications = notifications
            .iter()
            .map(|item| Notification {
                id: item.id.clone().into(),
                unread: item.unread,
                reason: item.reason.clone().into(),
                url: item.url.clone().into(),
                subject: Subject {
                    title: item.subject.title.clone().into(),
                    url: item.subject.url.clone().unwrap_or("".into()).into(),
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
                },
            })
            .collect::<Vec<Notification>>();

        mw.clone()
            .upgrade_in_event_loop(move |h| {
                let model = Rc::new(slint::VecModel::<Notification>::from(notifications));
                h.set_notifications_model(model.into());
            })
            .unwrap();
    }
}
