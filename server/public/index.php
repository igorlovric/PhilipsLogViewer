<?php
/**
 * Philips Log Viewer – shared pattern library API.
 *
 *   GET    /v1/ping
 *   GET    /v1/patterns[?since=REV]      all patterns (or those changed after REV), incl. withdrawn ones
 *   POST   /v1/patterns                  {"patterns":[{"uid","author","data"}]}  create / update (owner only)
 *   DELETE /v1/patterns/{uid}            withdraw (owner or admin)
 *
 * Headers: X-PLV-Key (API key), X-PLV-Owner (owner key of the installation, >= 32 chars),
 *          X-PLV-Admin (optional admin key).
 * All answers are JSON: {"ok":true,...} or {"ok":false,"error":"..."}.
 */

declare(strict_types=1);

const API_VERSION = 1;

$cfgFile = __DIR__ . '/../config.php';
if (!is_file($cfgFile)) {
    fail(500, 'server not configured (config.php missing)');
}
$cfg = require $cfgFile;

header('Content-Type: application/json; charset=utf-8');
header('Cache-Control: no-store');

// ------------------------------------------------------------------ helpers

function out(int $code, array $body): void
{
    http_response_code($code);
    header('Content-Type: application/json; charset=utf-8');
    echo json_encode($body, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    exit;
}

function fail(int $code, string $msg): void
{
    out($code, ['ok' => false, 'error' => $msg]);
}

function header_value(string $name): string
{
    $key = 'HTTP_' . strtoupper(str_replace('-', '_', $name));
    return trim((string)($_SERVER[$key] ?? ''));
}

function client_ip(): string
{
    return substr((string)($_SERVER['REMOTE_ADDR'] ?? ''), 0, 45);
}

function db(array $cfg): PDO
{
    static $pdo = null;
    if ($pdo === null) {
        try {
            $pdo = new PDO($cfg['db_dsn'], $cfg['db_user'], $cfg['db_pass'], [
                PDO::ATTR_ERRMODE => PDO::ERRMODE_EXCEPTION,
                PDO::ATTR_DEFAULT_FETCH_MODE => PDO::FETCH_ASSOC,
                PDO::ATTR_EMULATE_PREPARES => false,
            ]);
        } catch (Throwable $e) {
            fail(500, 'database not available');
        }
    }
    return $pdo;
}

function now(): string
{
    return gmdate('Y-m-d H:i:s');
}

function rate_limit(PDO $db, string $kind, int $limit): void
{
    if ($limit <= 0) {
        return;
    }
    $minute = intdiv(time(), 60);
    $ip = client_ip();
    $db->prepare('INSERT INTO plv_rate (ip, kind, minute, hits) VALUES (?, ?, ?, 1)
                  ON DUPLICATE KEY UPDATE hits = hits + 1')->execute([$ip, $kind, $minute]);
    $st = $db->prepare('SELECT hits FROM plv_rate WHERE ip = ? AND kind = ? AND minute = ?');
    $st->execute([$ip, $kind, $minute]);
    $hits = (int)$st->fetchColumn();
    if (random_int(1, 50) === 1) {
        $db->prepare('DELETE FROM plv_rate WHERE minute < ?')->execute([$minute - 5]);
    }
    if ($hits > $limit) {
        fail(429, 'too many requests, try again in a minute');
    }
}

function next_rev(PDO $db): int
{
    $db->exec('UPDATE plv_counter SET v = LAST_INSERT_ID(v + 1) WHERE id = 1');
    return (int)$db->query('SELECT LAST_INSERT_ID()')->fetchColumn();
}

function history(PDO $db, string $uid, int $rev, string $action, bool $admin, string $author, string $checksum, string $data): void
{
    $db->prepare('INSERT INTO plv_history (uid, rev, action, by_admin, author, checksum, data, ip, created_at)
                  VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)')
       ->execute([$uid, $rev, $action, $admin ? 1 : 0, $author, $checksum, $data, client_ip(), now()]);
}

function is_uid(string $s): bool
{
    return (bool)preg_match('/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/', $s);
}

function owner_hash(): string
{
    $owner = header_value('X-PLV-Owner');
    if (strlen($owner) < 32 || strlen($owner) > 200) {
        fail(400, 'missing or invalid owner key');
    }
    return hash('sha256', $owner);
}

function is_admin(array $cfg): bool
{
    $k = header_value('X-PLV-Admin');
    if ($k === '') {
        return false;
    }
    foreach ($cfg['admin_keys'] ?? [] as $a) {
        if ($a !== '' && hash_equals((string)$a, $k)) {
            return true;
        }
    }
    return false;
}

// ------------------------------------------------------------------ routing

$method = $_SERVER['REQUEST_METHOD'] ?? 'GET';
$path = (string)parse_url((string)($_SERVER['REQUEST_URI'] ?? '/'), PHP_URL_PATH);
$base = rtrim(str_replace('\\', '/', dirname((string)($_SERVER['SCRIPT_NAME'] ?? '/'))), '/');
if ($base !== '' && str_starts_with($path, $base)) {
    $path = substr($path, strlen($base));
}
$path = '/' . trim($path, '/');

// API key
$keys = array_filter($cfg['api_keys'] ?? [], fn($k) => $k !== '');
if ($keys) {
    $given = header_value('X-PLV-Key');
    $okKey = false;
    foreach ($keys as $k) {
        if (hash_equals((string)$k, $given)) {
            $okKey = true;
        }
    }
    if (!$okKey) {
        fail(401, 'invalid API key');
    }
}

if ($path === '/v1/ping' && $method === 'GET') {
    out(200, ['ok' => true, 'server' => 'plv', 'api' => API_VERSION, 'time' => now()]);
}

if ($path === '/v1/patterns' && $method === 'GET') {
    list_patterns($cfg);
}

if ($path === '/v1/patterns' && $method === 'POST') {
    save_patterns($cfg);
}

if (preg_match('#^/v1/patterns/([0-9a-f-]{36})$#', $path, $m) && $method === 'DELETE') {
    withdraw_pattern($cfg, $m[1]);
}

fail(404, 'unknown request');

// ------------------------------------------------------------------ handlers

function list_patterns(array $cfg): void
{
    $db = db($cfg);
    rate_limit($db, 'r', (int)($cfg['rate_read'] ?? 120));
    $since = max(0, (int)($_GET['since'] ?? 0));
    $owner = header_value('X-PLV-Owner');
    $mine = strlen($owner) >= 32 ? hash('sha256', $owner) : '';

    $st = $db->prepare('SELECT uid, owner_hash, author, name, checksum, data, rev, deleted, created_at, updated_at
                        FROM plv_patterns WHERE rev > ? ORDER BY rev');
    $st->execute([$since]);
    $list = [];
    foreach ($st as $r) {
        $list[] = [
            'uid' => $r['uid'],
            'author' => $r['author'],
            'name' => $r['name'],
            'checksum' => $r['checksum'],
            'rev' => (int)$r['rev'],
            'deleted' => (bool)$r['deleted'],
            'mine' => $mine !== '' && hash_equals($r['owner_hash'], $mine),
            'created_at' => $r['created_at'],
            'updated_at' => $r['updated_at'],
            'data' => $r['deleted'] ? '' : $r['data'],
        ];
    }
    $rev = (int)$db->query('SELECT v FROM plv_counter WHERE id = 1')->fetchColumn();
    out(200, ['ok' => true, 'rev' => $rev, 'patterns' => $list]);
}

function save_patterns(array $cfg): void
{
    $db = db($cfg);
    rate_limit($db, 'w', (int)($cfg['rate_write'] ?? 60));
    $owner = owner_hash();
    $admin = is_admin($cfg);
    $maxBytes = (int)($cfg['max_pattern_bytes'] ?? 65536);
    $maxCount = (int)($cfg['max_patterns_per_request'] ?? 200);

    $raw = file_get_contents('php://input', false, null, 0, $maxBytes * $maxCount + 65536);
    $body = json_decode((string)$raw, true);
    if (!is_array($body) || !isset($body['patterns']) || !is_array($body['patterns'])) {
        fail(400, 'expected {"patterns":[...]}');
    }
    if (count($body['patterns']) > $maxCount) {
        fail(413, "at most $maxCount patterns per request");
    }

    $results = [];
    foreach ($body['patterns'] as $p) {
        $uid = is_array($p) ? strtolower((string)($p['uid'] ?? '')) : '';
        $data = is_array($p) ? (string)($p['data'] ?? '') : '';
        $author = is_array($p) ? mb_substr(trim((string)($p['author'] ?? '')), 0, 100) : '';
        if (!is_uid($uid)) {
            $results[] = ['uid' => $uid, 'status' => 'invalid', 'error' => 'invalid uid'];
            continue;
        }
        if (strlen($data) > $maxBytes) {
            $results[] = ['uid' => $uid, 'status' => 'invalid', 'error' => 'pattern too large'];
            continue;
        }
        $obj = json_decode($data, true);
        $name = is_array($obj) ? trim((string)($obj['name'] ?? '')) : '';
        $steps = is_array($obj) ? ($obj['steps'] ?? null) : null;
        if ($name === '' || !is_array($steps) || count($steps) < 1 || count($steps) > 1000) {
            $results[] = ['uid' => $uid, 'status' => 'invalid', 'error' => 'not a valid pattern'];
            continue;
        }
        $name = mb_substr($name, 0, 200);
        $checksum = hash('sha256', $data);

        $db->beginTransaction();
        try {
            $st = $db->prepare('SELECT owner_hash, checksum, deleted FROM plv_patterns WHERE uid = ? FOR UPDATE');
            $st->execute([$uid]);
            $cur = $st->fetch();
            if ($cur && !hash_equals($cur['owner_hash'], $owner) && !$admin) {
                $db->rollBack();
                $results[] = ['uid' => $uid, 'status' => 'forbidden', 'error' => 'this pattern belongs to someone else'];
                continue;
            }
            if ($cur && $cur['checksum'] === $checksum && !$cur['deleted']) {
                $db->rollBack();
                $results[] = ['uid' => $uid, 'status' => 'unchanged', 'checksum' => $checksum];
                continue;
            }
            $rev = next_rev($db);
            if ($cur) {
                $db->prepare('UPDATE plv_patterns SET author = ?, name = ?, checksum = ?, data = ?, rev = ?, deleted = 0, updated_at = ?
                              WHERE uid = ?')->execute([$author, $name, $checksum, $data, $rev, now(), $uid]);
                $action = $cur['deleted'] ? 'restore' : 'update';
            } else {
                $db->prepare('INSERT INTO plv_patterns (uid, owner_hash, author, name, checksum, data, rev, deleted, created_at, updated_at)
                              VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)')
                   ->execute([$uid, $owner, $author, $name, $checksum, $data, $rev, now(), now()]);
                $action = 'create';
            }
            history($db, $uid, $rev, $action, $admin && $cur && !hash_equals($cur['owner_hash'], $owner), $author, $checksum, $data);
            $db->commit();
            $results[] = ['uid' => $uid, 'status' => $action === 'create' ? 'created' : 'updated', 'rev' => $rev, 'checksum' => $checksum];
        } catch (Throwable $e) {
            if ($db->inTransaction()) {
                $db->rollBack();
            }
            $results[] = ['uid' => $uid, 'status' => 'error', 'error' => 'database error'];
        }
    }
    out(200, ['ok' => true, 'results' => $results]);
}

function withdraw_pattern(array $cfg, string $uid): void
{
    $db = db($cfg);
    rate_limit($db, 'w', (int)($cfg['rate_write'] ?? 60));
    $admin = is_admin($cfg);
    $owner = $admin && header_value('X-PLV-Owner') === '' ? '' : owner_hash();

    $db->beginTransaction();
    $st = $db->prepare('SELECT owner_hash, author, checksum, data, deleted FROM plv_patterns WHERE uid = ? FOR UPDATE');
    $st->execute([$uid]);
    $cur = $st->fetch();
    if (!$cur) {
        $db->rollBack();
        fail(404, 'no such pattern');
    }
    $isOwner = $owner !== '' && hash_equals($cur['owner_hash'], $owner);
    if (!$isOwner && !$admin) {
        $db->rollBack();
        fail(403, 'only the author (or an admin) can withdraw this pattern');
    }
    if ($cur['deleted']) {
        $db->rollBack();
        out(200, ['ok' => true, 'status' => 'withdrawn']);
    }
    $rev = next_rev($db);
    $db->prepare('UPDATE plv_patterns SET deleted = 1, rev = ?, updated_at = ? WHERE uid = ?')->execute([$rev, now(), $uid]);
    history($db, $uid, $rev, 'withdraw', !$isOwner, $cur['author'], $cur['checksum'], $cur['data']);
    $db->commit();
    out(200, ['ok' => true, 'status' => 'withdrawn', 'rev' => $rev]);
}
