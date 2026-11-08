FROM node:24-alpine AS frontend
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY tsconfig.json ./
COPY web ./web
COPY public ./public
RUN npm run build
FROM rust:alpine AS backend
RUN apk add --no-cache musl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release
FROM alpine:3.23
WORKDIR /app
RUN addgroup -g 1000 poker && adduser -D -u 1000 -G poker poker && mkdir /data && chown poker:poker /data
COPY --from=backend /app/target/release/poker-bank /app/poker-bank
COPY --from=frontend /app/public /app/public
USER poker
ENV PORT=8080 DB_PATH=/data/poker.db
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s CMD wget -q -O /dev/null http://127.0.0.1:8080/healthz || exit 1
CMD ["/app/poker-bank"]
