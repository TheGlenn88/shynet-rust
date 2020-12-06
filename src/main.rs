use analytics::startup::run;
use dotenv::dotenv;
use sqlx::PgPool;

#[macro_use]
extern crate dotenv_codegen;

use analytics::mobc_pool;
use std::net::TcpListener;

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let mobc_pool = mobc_pool::connect().await.expect("can create mobc pool");
    let connection = PgPool::connect(&dotenv!("DATABASE_URL"))
        .await
        .expect("Failed to connect to Postgres.");
    let address = format!("0.0.0.0:{}", dotenv!("APP_PORT"));
    let listener = TcpListener::bind(address)?;
    run(listener, connection, mobc_pool)?.await
}
