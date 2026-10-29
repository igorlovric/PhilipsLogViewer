# Philips Log Viewer – shared pattern server

A small API (PHP 8 + MySQL/MariaDB, Apache) that stores the patterns users share from the program. No web interface.

```
server/
├── public/            ← the web server's DocumentRoot points HERE (only this folder is reachable)
│   ├── index.php      the whole API
│   └── .htaccess      sends every request to index.php
├── config.example.php copy to config.php and fill in (config.php is not in git)
├── schema.sql         database tables
└── .htaccess          denies access if the web server points to server/ by mistake
```

## Installation (once)

1. **Get the code** on the server, e.g. only the `server/` folder:
   ```bash
   git clone --filter=blob:none --sparse https://github.com/igorlovric/PhilipsLogViewer.git /var/www/plv
   cd /var/www/plv && git sparse-checkout set server
   ```
   Later updates: `cd /var/www/plv && git pull`.

2. **Database**:
   ```sql
   CREATE DATABASE plv CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;
   CREATE USER 'plv'@'localhost' IDENTIFIED BY 'a-strong-password';
   GRANT SELECT, INSERT, UPDATE, DELETE ON plv.* TO 'plv'@'localhost';
   ```
   ```bash
   mysql -u root -p plv < /var/www/plv/server/schema.sql
   ```

3. **Config**: `cp server/config.example.php server/config.php` and fill in the database password,
   the API key and your admin key. Generate keys e.g. with `openssl rand -hex 24`.

4. **Apache** (virtual host for plv.shadowdreams.net, HTTPS with Let's Encrypt):
   ```apache
   <VirtualHost *:443>
       ServerName plv.shadowdreams.net
       DocumentRoot /var/www/plv/server/public
       <Directory /var/www/plv/server/public>
           AllowOverride All
           Require all granted
       </Directory>
       # SSLEngine / certificate lines from certbot
   </VirtualHost>
   ```
   `mod_rewrite` must be on (`a2enmod rewrite`). Check: `https://plv.shadowdreams.net/v1/ping`
   with the header `X-PLV-Key` → `{"ok":true,...}`; without the key → `invalid API key`.

5. **Key for the program**: in GitHub → repository → Settings → Secrets and variables → Actions →
   *New repository secret* `PLV_API_KEY` = the same API key as in `config.php`.
   Every release built after that has the key built in.

6. **Admin** (optional): put your admin key into your own `PhilipsLogViewer.config`:
   `"share_admin_key": "…"` – ☰ → Shared patterns then has a tab *All on server (admin)*.

For testing another server: `"share_server": "https://test.example.com"` and `"share_api_key": "…"` in `PhilipsLogViewer.config`.

## Updating the server

```bash
cd /var/www/plv && git pull
mysql -u root -p plv < server/schema.sql   # adds new tables, existing data stays (CREATE TABLE IF NOT EXISTS)
```

## API

All requests: header `X-PLV-Key: <api key>`; writes also `X-PLV-Owner: <owner key>`
(each installation creates its own, ≥ 32 characters; the server stores only its SHA-256).
Answers are JSON, `{"ok":true,…}` or `{"ok":false,"error":"…"}`.

| Request | What it does |
|---|---|
| `GET /v1/ping` | check |
| `GET /v1/patterns[?since=REV]` | all patterns (or changed after REV), withdrawn ones with `"deleted":true`; `mine` = belongs to the sent owner key |
| `POST /v1/patterns` | `{"patterns":[{"uid","author","data"}]}` – create or update (only the owner); `data` = pattern JSON as text, `checksum` = its SHA-256 |
| `DELETE /v1/patterns/{uid}` | withdraw (owner, or admin with `X-PLV-Admin`) |
| `GET /v1/products` | product number → name, learned by the programs from opened logs |
| `POST /v1/products` | `{"products":[{"id","name"}]}` – add / update (history in `plv_product_history`) |

Nothing is ever deleted physically: withdrawn patterns keep their data, and `plv_history` holds every version
(create / update / withdraw / restore, with IP and time), so anything can be looked up or restored by hand.

Limits (config.php): requests per IP and minute, pattern size, patterns per request.
