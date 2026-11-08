FROM node:24-alpine
WORKDIR /app
COPY package*.json ./
RUN npm ci --omit=dev && npm cache clean --force
COPY server.js engine.js ./
COPY public ./public
COPY deploy/backup.mjs ./backup.mjs
RUN mkdir /data && chown node:node /data
USER node
ENV NODE_ENV=production PORT=8080 DB_PATH=/data/poker.db
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s CMD node -e "fetch('http://127.0.0.1:8080/healthz').then(r=>{if(!r.ok)process.exit(1)}).catch(()=>process.exit(1))"
CMD ["node","server.js"]
