use analytics::configuration::get_configuration;
use analytics::startup::run;
use dotenv::dotenv;
use sqlx::PgPool;

use analytics::mobc_pool;
use std::net::TcpListener;

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let configuration = get_configuration().expect("Failed to read configuration.");
    let mobc_pool = mobc_pool::connect().await.expect("can create mobc pool");
    let connection = PgPool::connect(&configuration.database.connection_string())
        .await
        .expect("Failed to connect to Postgres.");
    let address = format!("0.0.0.0:{}", configuration.application_port);
    let listener = TcpListener::bind(address)?;
    run(listener, connection, mobc_pool)?.await
}
