
mod models;

use tokio::fs;
use dotenv::dotenv;
use std::{
    env,
    path::PathBuf,
    io::ErrorKind::AlreadyExists,
};
use std::net::SocketAddr;
use tower_http::services::ServeDir;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use axum::{
    extract::{Query, Multipart, TypedHeader},
    headers::{authorization::Bearer, Authorization},
    response::{Response, IntoResponse},
    http::StatusCode,
    routing::{get, post, get_service},
    response::Html,
    body::Body,
    Router,
    Json,
};

const CDN_URL: &str = "https://cdn.tomthebomb.dev";
const PORT: u16 = 8030;


pub fn generate_filename() -> String {
    let mut rng = thread_rng();

    (0..10)
        .map(|_| rng.sample(Alphanumeric) as char)
        .collect::<String>()
}


async fn post_upload(
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Query(query): Query<models::DirectoryQuery>,
    mut multipart: Multipart,
) -> Response {
    // handler for POST /upload, for uploading to the cdn

    if let Ok(auth_token) = env::var("auth") {
        if auth.token() != auth_token {
            return (
                StatusCode::UNAUTHORIZED,
                "Incorrect authorization token"
            ).into_response()
        } else {
            if let Ok(Some(field)) = multipart.next_field().await {
                let filename = field.file_name()
                    .map(|s| s.to_string())
                    .unwrap_or_else(generate_filename);

                let path = PathBuf::from(&format!(
                    "./uploads/{}/{}",
                    query.directory
                        .unwrap_or_else(|| "".to_string())
                        .trim_matches('/'),
                    filename,
                ));

                if let Some(parent) = path.parent() {
                    if let Err(err) = fs::create_dir_all(parent).await {
                        if err.kind() != AlreadyExists {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Creating the directory failed",
                            ).into_response()
                        }
                    }
                }

                if let Ok(bytes) = field.bytes().await {
                    if let Err(_) = fs::write(&path, bytes).await {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Writing to file system failed",
                        ).into_response()
                    }
                } else {
                    return (
                        StatusCode::BAD_REQUEST,
                        "Improper bytes sent",
                    ).into_response()
                }

                let path_string = path.display()
                    .to_string()
                    .trim_start_matches(".")
                    .to_string();
                (
                    StatusCode::OK,
                    Json(models::UploadResponse {
                        full_url: format!("{}{}", CDN_URL, path_string),
                        path: path_string,
                        filename,
                    })
                ).into_response()
            } else {
                (
                    StatusCode::BAD_REQUEST,
                    "Missing image field in the multipart form"
                ).into_response()
            }
        }
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to get auth token from env"
        ).into_response()
    }
}


async fn get_root() -> Html<String> {
    Html("CDN Home Page".to_string())
}


async fn run(app: Router<Body>, port: u16) {
    // runs the webserver
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let server = axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to await for SIGINT")
        });

    println!("[Server Initialized]");
    server.await.expect("Failed to start server");
}


#[tokio::main]
async fn main() {
    drop(dotenv());

    fs::create_dir("./uploads")
        .await
        .unwrap_or_else(|err| {
            match err.kind() {
                AlreadyExists => (),
                _ => panic!("{:?}", err),
            }
        });

    let app: Router<Body> = Router::new()
        .route("/", get(get_root))
        .route("/upload", post(post_upload))
        .fallback(get_service(ServeDir::new("./uploads"))
            .handle_error(|err| async move {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to serve files: {}", err),
                )
            })
        );

    run(app, PORT).await;
}