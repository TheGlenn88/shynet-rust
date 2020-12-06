FROM rust:1.48 as builder

WORKDIR /shynet-rust
COPY ./ ./

ENV SQLX_OFFLINE true

RUN cargo build --release

FROM debian:buster-slim
ARG APP=/usr/src/app

RUN apt-get update \
    && apt-get install -y ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

EXPOSE 8000

ENV TZ=Etc/UTC \
    APP_USER=appuser

RUN groupadd $APP_USER \
    && useradd -g $APP_USER $APP_USER \
    && mkdir -p ${APP} \
    && mkdir -p ${APP}/templates/analytics \
    && mkdir -p ${APP}/analytics 

COPY --from=builder /shynet-rust/target/release/shynet-rust ${APP}/shynet-rust
COPY --from=builder /shynet-rust/templates/analytics/script.js ${APP}/templates/analytics/script.js
COPY --from=builder /shynet-rust/templates/analytics/script.js ${APP}/analytics/script.js

RUN chown -R $APP_USER:$APP_USER ${APP}

USER $APP_USER
WORKDIR ${APP}

CMD ["./shynet-rust"]