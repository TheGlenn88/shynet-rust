use crate::routes::{
    health_check, ingress_pixel_get, ingress_script_get, ingress_script_post,
};

use actix_cors::Cors;
use actix_web::dev::Server;
use actix_web::{web, App, HttpServer};
use listenfd::ListenFd;
use sqlx::PgPool;
use std::net::TcpListener;
use tera::Tera;
use mobc_redis::{RedisConnectionManager};
use mobc::{Pool};

pub type MobcPool = Pool<RedisConnectionManager>;

pub struct AppData {
    pub tmpl: Tera,
}

pub fn run(listener: TcpListener, db_pool: PgPool, mobc_pool: MobcPool) -> Result<Server, std::io::Error> {
    let db_pool = web::Data::new(db_pool);
    let mobc_pool = web::Data::new(mobc_pool);
    let mut listenfd = ListenFd::from_env();
    let server = HttpServer::new(move || {
        let tera = Tera::new(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/**/*")).unwrap();

        App::new()
            .service(
                web::resource("/ingress/{site_uuid}/script.js")
                    .wrap(Cors::new().allowed_methods(vec!["GET", "POST"]).finish())
                    .route(web::get().to(ingress_script_get))
                    .route(web::post().to(ingress_script_post)),
            )
            .service(
                web::resource("/ingress/{site_uuid}/pixel.gif")
                    .wrap(Cors::new().allowed_methods(vec!["GET"]).finish())
                    .route(web::get().to(ingress_pixel_get)),
            )
            .data(AppData { tmpl: tera })
            .route("/health_check", web::get().to(health_check))
            .app_data(db_pool.clone())
            .app_data(mobc_pool.clone())
    });

    if let Some(l) = listenfd.take_tcp_listener(0).unwrap() {
        Ok(server.listen(l)?.run())
    } else {
        Ok(server.listen(listener)?.run())
    }
}
