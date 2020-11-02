use actix_web::http::header::{DNT, USER_AGENT};
use actix_web::{web, HttpRequest, HttpResponse};
use base64;
use chrono::{Duration, Utc};
use hex;
use mobc::Pool;
use mobc_redis::RedisConnectionManager;
use sha2::{Digest, Sha256};
use sql_builder::prelude::*;
use sqlx::types::ipnetwork::IpNetwork;
use sqlx::PgPool;
use std::error::Error;
use tera::Context;
use uuid::Uuid;

use crate::mobc_pool;

use crate::{db_error::DatabaseError, startup::AppData};

pub type MobcPool = Pool<RedisConnectionManager>;

use crate::configuration::get_configuration;

#[derive(
    Default,
    Debug,
    Clone,
    PartialEq,
    serde_derive::Serialize,
    serde_derive::Deserialize,
    sqlx::FromRow,
)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    #[serde(with = "my_uuid")]
    pub uuid: Uuid,
    pub name: String,
    pub link: String,
    pub origins: String,
    pub status: String,
    pub owner_id: i32,
    pub respect_dnt: bool,
    pub collect_ips: bool,
    pub ignored_ips: String,
    pub hide_referrer_regex: String,
    pub ignore_robots: bool,
    pub script_inject: String,
}

#[derive(
    Default,
    Debug,
    Clone,
    PartialEq,
    serde_derive::Serialize,
    serde_derive::Deserialize,
    sqlx::FromRow,
)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSession {
    #[serde(with = "my_uuid")]
    pub uuid: Uuid,
    pub identifier: String,
    pub start_time: String,
    pub last_seen: String,
    pub user_agent: String,
    pub browser: i32,
    pub device: bool,
    pub device_type: bool,
    pub os: String,
    pub ip: String,
    pub asn: bool,
    pub country: String,
    pub time_zone: String,
    pub service_id: String,
}

mod my_uuid {
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};
    use std::str::FromStr;
    use uuid::Uuid;

    pub fn serialize<S>(val: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        val.to_string().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Uuid, D::Error>
    where
        D: Deserializer<'de>,
    {
        let val: &str = Deserialize::deserialize(deserializer)?;
        Uuid::from_str(val).map_err(D::Error::custom)
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngressData {
    idempotency: String,
    referrer: String,
    location: String,
    load_time: u32,
}

#[derive(serde::Serialize)]
pub struct ValidationError {
    error: String,
}

#[derive(std::fmt::Debug)]
struct Sess {
    id: Uuid,
}

async fn get_site(connection: web::Data<PgPool>, uuid: Uuid) -> Result<Site, Box<dyn Error>> {
    let sql = SqlBuilder::select_from("core_service")
        .fields(&[
            "uuid",
            "name",
            "link",
            "origins",
            "status",
            "owner_id",
            "respect_dnt",
            "collect_ips",
            "ignored_ips",
            "hide_referrer_regex",
            "ignore_robots",
            "script_inject",
        ])
        .and_where("uuid = ?".bind(&uuid.to_string()))
        .sql()?;

    let site = sqlx::query_as::<_, Site>(&sql)
        .fetch_one(connection.get_ref())
        .await?;

    Ok(site)
}

pub async fn ingress_script_post(
    req: HttpRequest,
    data: web::Json<IngressData>,
    connection: web::Data<PgPool>,
    mobc_pool: web::Data<MobcPool>,
) -> Result<HttpResponse, HttpResponse> {
    let service_id = Uuid::parse_str(req.match_info().get("site_uuid").unwrap());

    let site = get_site(connection.clone(), service_id.clone().unwrap()).await;
    if let Err(_) = site {
        return Ok(HttpResponse::NotFound().finish());
    }

    let idempotency = &data.idempotency;
    let referrer = &data.referrer;
    let location = &data.location;
    let load_time = &data.load_time;
    let _user_agent = req.headers().get(USER_AGENT);
    let dnt = req.headers().get(DNT);

    // Respect DNT
    if let Some(d) = dnt {
        if d == "1" {
            return Ok(HttpResponse::Ok().finish());
        }
    }

    // Create or Update Session
    // Get a hash of the IP and User Agent
    let mut hasher = Sha256::new();

    // get Connection info from request
    let conn_info = req.connection_info();

    // split the ip address from the port
    let ip: Vec<&str> = conn_info.realip_remote_addr().unwrap().split(":").collect();

    // create IpNetwork by parsing the ip address
    let v4_net: IpNetwork = ip[0].parse().unwrap();

    // hash the concatenated ip + user agent
    hasher.update(format!("{}{}", ip[0], get_user_agent(&req).unwrap()));

    // create a hex string of the hash
    let request_hash = hex::encode(hasher.finalize());

    let time = Utc::now();

    let session_uuid_select = sqlx::query!(
        "SELECT uuid FROM analytics_session WHERE identifier = $1 and start_time > $2",
        request_hash,
        time.checked_sub_signed(Duration::seconds(1800))
    )
    .fetch_all(connection.get_ref())
    .await;

    let uuid = Uuid::new_v4();

    if let Ok(s) = session_uuid_select {
        if s.len() > 0 {
            let fetched_uuid = s[0].uuid;
            println!("{:?}", fetched_uuid);
        } else {

            let result = sqlx::query!(
                r#"
                INSERT INTO analytics_session (uuid, identifier, start_time, last_seen, user_agent, browser, device, device_type, os, ip, asn, country, time_zone, service_id)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                "#,
                uuid,
                request_hash,
                time,
                time,
                "",
                "",
                "",
                "",
                "",
                v4_net,
                "",
                "",
                "",
                service_id.unwrap()
            )
                .execute(connection.get_ref())
                .await
                .map_err(|e| {
                    println!("Failed to execute query: {}", e);
                    HttpResponse::InternalServerError().finish()
                });

            if let Err(r) = result {
                println!("{:?}", r);
            }
        }
    } else {
        println!("miss");
    }

    // Create or update hit
    if idempotency.len() > 0 {
        println!("idem GT 0 {}", idempotency);

        // Check the cache
        let hit_cache = mobc_pool::get_str(
            &mobc_pool,
            ("hit_idempotency_".to_string() + idempotency).as_str(),
        )
        .await;

        if let Ok(b) = hit_cache {
            println!(
                "I hit the cache, lets update the heartbeat {}, {}",
                idempotency.len(),
                b
            );
        }
        println!(
            "## idempotency {} ## referrer {} ## location {} ## load_time {}",
            idempotency, referrer, location, load_time
        );

        do_analytics_hit(connection.clone(), true, uuid, "JS", idempotency, mobc_pool).await;
    }

    Ok(HttpResponse::Ok().finish())
}

pub async fn ingress_script_get(
    data: web::Data<AppData>,
    req: HttpRequest,
) -> Result<HttpResponse, HttpResponse> {
    let configuration = get_configuration().expect("Failed to read configuration.");
    let site_uuid = req.match_info().get("site_uuid").unwrap();

    let mut ctx = Context::new();
    //TODO: make site_uuid check for a valid site
    ctx.insert("site_uuid", site_uuid);
    ctx.insert("app_url", &configuration.app_url);

    //TODO: make heartbeat configurable
    ctx.insert("heartbeat_frequency", &5000);
    let rendered = data.tmpl.render("analytics/script.js", &ctx).unwrap();

    // TODO: log a session
    Ok(HttpResponse::Ok().body(rendered))
}

pub async fn ingress_pixel_get(
    req: HttpRequest,
    connection: web::Data<PgPool>,
) -> Result<HttpResponse, HttpResponse> {
    let dnt = req.headers().get(DNT);

    // Respect DNT
    if let Some(d) = dnt {
        if d == "1" {
            return Ok(HttpResponse::Ok().finish());
        }
    }

    // Get a hash of the IP and User Agent
    let mut hasher = Sha256::new();

    // get Connection info from request
    let conn_info = req.connection_info();

    // split the ip address from the port
    let ip: Vec<&str> = conn_info.realip_remote_addr().unwrap().split(":").collect();

    // create IpNetwork by parsing the ip address
    let v4_net: IpNetwork = ip[0].parse().unwrap();

    // hash the concatenated ip + user agent
    hasher.update(format!("{}{}", ip[0], get_user_agent(&req).unwrap()));

    // create a hex string of the hash
    let request_hash = hex::encode(hasher.finalize());

    let time = Utc::now();

    let service_id = Uuid::parse_str(req.match_info().get("site_uuid").unwrap());

    let session_uuid_select = sqlx::query!(
        "SELECT uuid FROM analytics_session WHERE identifier = $1 and start_time > $2",
        request_hash,
        time.checked_sub_signed(Duration::seconds(1800))
    )
    .fetch_all(connection.get_ref())
    .await;

    if let Ok(s) = session_uuid_select {
        if s.len() > 0 {
            let fetched_uuid = s[0].uuid;
            println!("{:?}", fetched_uuid);
        // do_analytics_hit(connection.clone(), false, fetched_uuid, "PIXEL", "").await;
        } else {
            let uuid = Uuid::new_v4();
            let result = sqlx::query!(
                r#"
                INSERT INTO analytics_session (uuid, identifier, start_time, last_seen, user_agent, browser, device, device_type, os, ip, asn, country, time_zone, service_id)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
                "#,
                uuid,
                request_hash,
                time,
                time,
                "",
                "",
                "",
                "",
                "",
                v4_net,
                "",
                "",
                "",
                service_id.unwrap()
            )
                .execute(connection.get_ref())
                .await
                .map_err(|e| {
                    println!("Failed to execute query: {}", e);
                    HttpResponse::InternalServerError().finish()
                });

            if let Err(r) = result {
                println!("{:?}", r);
            }

            // do_analytics_hit(connection.clone(), true, uuid, "PIXEL", "".as_string()).await;
        }
    } else {
        println!("miss");
    }

    let data = base64::decode("R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==");

    Ok(HttpResponse::Ok()
        .content_type("image/gif")
        .header("Cache-Control", "no-cache, no-store, must-revalidate")
        .header("Access-Control-Allow-Origin", "*")
        .body(data.unwrap()))
}

fn get_user_agent<'a>(req: &'a HttpRequest) -> Option<&'a str> {
    req.headers().get(USER_AGENT)?.to_str().ok()
}

async fn do_analytics_hit(
    connection: web::Data<PgPool>,
    initial: bool,
    session_id: Uuid,
    tracker: &str,
    idempotency: &String,
    mobc_pool: web::Data<MobcPool>,
) {
    let time = Utc::now();
    let hit_id = sqlx::query!(
    r#"
    INSERT INTO analytics_hit (initial, start_time, last_seen, heartbeats, tracker, location, referrer, load_time, session_id)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
    RETURNING id
    "#,
    initial,
    time,
    time,
    1,
    tracker,
    "",
    "",
    0.0,
    session_id
    )
        .fetch_one(connection.get_ref())
        .await
        .map_err(|e| {
            println!("Failed to execute query: {}", e);
            HttpResponse::InternalServerError().finish()
        });

    // println!("{:?}", hit_id.unwrap().id);

    let result = mobc_pool::set_str(
        &mobc_pool,
        ("hit_idempotency_".to_string() + idempotency).as_str(),
        hit_id.unwrap().id.to_string().as_str(),
        60usize,
    )
    .await
    .map_err(|e| {
        println!("Failed to execute query: {:?}", e);
        HttpResponse::InternalServerError().finish()
    });

    if let Err(r) = result {
        println!("{:?}", r);
    }
}
