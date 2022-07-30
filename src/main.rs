
use dotenv::dotenv;
use tokio::fs;
use std::{
    io::ErrorKind::{AlreadyExists, NotFound},
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    env,
};

use tower_http::services::{ServeDir, ServeFile};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use axum::{
    headers::{authorization::Bearer, Authorization},
    routing::{get, post, delete, get_service},
    extract::{Path, Query, Multipart, TypedHeader},
    response::{Response, IntoResponse},
    http::StatusCode,
    response::Html,
    body::Body,
    Router,
    Json,
};

mod models;

const CDN_URL: &str = "https://cdn.tomthebomb.dev";
const MAX_FILE_SIZE: usize = 30_000_000;
const PORT: u16 = 8030;


fn generate_filename() -> String {
    // generates a 10 character long random alphanumeric string
    // acting like a fallback filename
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
                "Incorrect authorization token",
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
                    if bytes.len() > MAX_FILE_SIZE {
                        return (
                            StatusCode::PAYLOAD_TOO_LARGE,
                            format!("uploaded files cannot exceed the limit of {} bytes", MAX_FILE_SIZE),
                        ).into_response()
                    }

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
                    .trim_start_matches("./uploads")
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
                    "Missing image field in the multipart form",
                ).into_response()
            }
        }
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to get auth token from env",
        ).into_response()
    }
}


async fn delete_file(
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Path(mut path): Path<String>,
) -> Response {
    if let Ok(auth_token) = env::var("auth") {
        if auth.token() != auth_token {
            return (
                StatusCode::UNAUTHORIZED,
                "Incorrect authorization token",
            ).into_response()
        } else {
            path = format!(
                "./uploads/{}",
                path.trim_matches('/'),
            );

            if let Err(err) = fs::remove_file(path).await {
                match err.kind() {
                    NotFound => (
                        StatusCode::NOT_FOUND,
                        "The requested file was not found on the CDN",
                    ),
                    _ => (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Something went wrong when deleting the file",
                    )
                }.into_response()
            } else {
                let json: HashMap<&str, &str> = HashMap::from([
                    ("message", "File successfully deleted")
                ]);

                (
                    StatusCode::OK,
                    Json(json),
                ).into_response()
            }
        }
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to get auth token from env",
        ).into_response()
    }
}


async fn get_root() -> Html<&'static str> {
    // renders the home page
    Html(include_str!("../static/index.html"))
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
        .route("/delete/*path", delete(delete_file))
        .fallback(
            get_service(
                ServeDir::new("./uploads")
                    .fallback(
                        ServeDir::new("./static/")
                        .fallback(ServeFile::new("./static/notfound.html"))
                    )
            )
            .handle_error(|err| async move {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to serve CDN files: {}", err),
                )
            })
        );

    run(app, PORT).await;
}