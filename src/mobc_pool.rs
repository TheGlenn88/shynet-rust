use std::time::Duration;

use mobc::{Connection, Pool};
use mobc_redis::{redis, RedisConnectionManager};
use mobc_redis::redis::{AsyncCommands, FromRedisValue};
use thiserror::Error;

pub type MobcPool = Pool<RedisConnectionManager>;
pub type MobcCon = Connection<RedisConnectionManager>;

const CACHE_POOL_MAX_OPEN: u64 = 16;
const CACHE_POOL_MAX_IDLE: u64 = 8;
const CACHE_POOL_TIMEOUT_SECONDS: u64 = 1;
const CACHE_POOL_EXPIRE_SECONDS: u64 = 60;

const REDIS_CON_STRING: &str = "redis://127.0.0.1/";

type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("mobc error: {0}")]
    MobcError(#[from] MobcError),
}

#[derive(Error, Debug)]
pub enum MobcError {
    #[error("could not get redis connection from pool : {0}")]
    RedisPoolError(mobc::Error<mobc_redis::redis::RedisError>),
    #[error("error parsing string from redis result: {0}")]
    RedisTypeError(mobc_redis::redis::RedisError),
    #[error("error executing redis command: {0}")]
    RedisCMDError(mobc_redis::redis::RedisError),
    #[error("error creating Redis client: {0}")]
    RedisClientError(mobc_redis::redis::RedisError),
}

pub async fn connect() -> Result<MobcPool> {
    let client = redis::Client::open(REDIS_CON_STRING).map_err(MobcError::RedisClientError)?;
    let manager = RedisConnectionManager::new(client);
    Ok(Pool::builder()
        .get_timeout(Some(Duration::from_secs(CACHE_POOL_TIMEOUT_SECONDS)))
        .max_open(CACHE_POOL_MAX_OPEN)
        .max_idle(CACHE_POOL_MAX_IDLE)
        .max_lifetime(Some(Duration::from_secs(CACHE_POOL_EXPIRE_SECONDS)))
        .build(manager))
}

async fn get_con(pool: &MobcPool) -> Result<MobcCon> {
    pool.get().await.map_err(|e| {
        eprintln!("error connecting to redis: {}", e);
        MobcError::RedisPoolError(e).into()
    })
}

pub async fn set_str(pool: &MobcPool, key: &str, value: &str, ttl_seconds: usize) -> Result<()> {
    let mut con = get_con(&pool).await?;
    con.set(key, value).await.map_err(MobcError::RedisCMDError)?;
    if ttl_seconds > 0 {
        con.expire(key, ttl_seconds).await.map_err(MobcError::RedisCMDError)?;
    }
    Ok(())
}

pub async fn get_str(pool: &MobcPool, key: &str) -> Result<String> {
    let mut con = get_con(&pool).await?;
    let value = con.get(key).await.map_err(MobcError::RedisCMDError)?;

    FromRedisValue::from_redis_value(&value).map_err(|e| MobcError::RedisTypeError(e).into())
}

pub async fn exists(pool: &MobcPool, key: &str) -> Result<bool> {
    let mut con = get_con(&pool).await?;
    let value = con.exists(key).await.map_err(MobcError::RedisCMDError)?;

    Ok(value)
}

