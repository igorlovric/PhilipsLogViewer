<?php
// Copy this file to config.php (same folder, NOT inside public/) and fill in the values.
// config.php is not in git.
return [
    'db_dsn'  => 'mysql:host=localhost;dbname=plv;charset=utf8mb4',
    'db_user' => 'plv',
    'db_pass' => 'CHANGE-ME',

    // API keys the program may use (X-PLV-Key header). The key built into the program comes
    // from the GitHub secret PLV_API_KEY. An empty list = no key needed.
    'api_keys' => [
        'CHANGE-ME',
    ],

    // admin keys (X-PLV-Admin header, set as "share_admin_key" in PhilipsLogViewer.config):
    // may withdraw any pattern
    'admin_keys' => [
        'CHANGE-ME-TOO',
    ],

    // requests per IP and minute
    'rate_read'  => 120,
    'rate_write' => 60,

    // limits
    'max_pattern_bytes'  => 65536,
    'max_patterns_per_request' => 200,
];
