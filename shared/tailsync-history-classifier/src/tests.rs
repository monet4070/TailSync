use super::*;

fn category(text: &str) -> &'static str {
    classify_text(text).category
}

#[test]
fn classifies_deterministic_formats() {
    assert_eq!(category("https://example.com/docs?q=rust#intro"), "website");
    assert_eq!(category("http://localhost:5173/history"), "website");
    for website in [
        "www.example.com",
        "WWW.Example.XYZ/docs?q=rust#intro",
        "example.com",
        "sub.example.org/path",
        "baidu.com",
        "12306.cn",
        "github.io/project",
        "example.co.uk/docs",
        "example.com:8443/path",
        "localhost:5173",
        "192.168.1.1:8080/settings",
        "//example.com/docs",
    ] {
        assert_eq!(category(website), "website", "{website}");
    }
    assert_eq!(classify_text("https://example.com").confidence, 99);
    assert_eq!(classify_text("www.example.com").confidence, 97);
    assert_eq!(classify_text("example.com/docs").confidence, 96);
    assert_eq!(classify_text("example.com").confidence, 93);
    assert_eq!(
        category(r#"{"name":"TailSync","enabled":true}"#),
        "structured_data"
    );
    assert_eq!(category("[1, 2, 3]"), "structured_data");
    assert_eq!(category(r#"C:\Users\tester\notes.txt"#), "path");
    assert_eq!(
        category(r#"C:\Program Files (x86)\TailSync\TailSync.exe"#),
        "path"
    );
    assert_eq!(category("/Users/tester/Documents/notes.txt"), "path");
}

#[test]
fn classifies_commands_and_code() {
    assert_eq!(category("git status --short"), "command");
    assert_eq!(category("Get-Content README.md"), "command");
    assert_eq!(category("get-content README.md"), "command");
    assert_eq!(category("docker compose up -d"), "command");
    assert_eq!(category("go test ./..."), "command");
    assert_eq!(
        category(r#"C:\Tools\formatter.exe --check source.rs"#),
        "command"
    );
    assert_eq!(
        category(r#""C:\Program Files\TailSync\tailsync.exe" --version"#),
        "command"
    );
    assert_eq!(category("/usr/bin/git status --short"), "command");
    let nginx_deploy =
        "sudo tar -xzf /tmp/tailsync-site-theme-switch-20260728.tar.gz -C /var/www/tailsync\n\
sudo chown -R www-data:www-data /var/www/tailsync\n\
sudo nginx -t\n\
sudo systemctl reload nginx";
    let deployment_classification = classify_text(nginx_deploy);
    assert_eq!(deployment_classification.category, "command");
    assert_eq!(deployment_classification.confidence, 96);
    assert_eq!(deployment_classification.categories(), vec!["command"]);
    assert_eq!(category("git status --short\nnpm run build"), "command");
    assert_eq!(
        category("sudo customctl restart\nsudo customctl status"),
        "command"
    );
    assert_eq!(
        category("docker run --rm \\\n+  -v /tmp/source:/source \\\n+  alpine:latest"),
        "command"
    );
    assert_eq!(
        category("const ids = items.map((item) => item.id);"),
        "code"
    );
    assert_eq!(
        category("def greet(name):\n    return f\"Hello {name}\""),
        "code"
    );
    assert_eq!(
        category("SELECT id, name FROM users WHERE active = 1"),
        "code"
    );
    assert_eq!(category("console.log(\"ready\");"), "code");
    assert_eq!(category("const url = \"https://example.com\";"), "code");
    assert_eq!(category("#!/bin/sh\ngit status"), "code");
    assert_eq!(category("import os"), "code");
    assert_eq!(category("import numpy as np"), "code");
    assert_eq!(category("from pathlib import Path"), "code");
    assert_eq!(category("use std::collections::HashMap;"), "code");
    assert_eq!(category("#include <stdio.h>"), "code");
    assert_eq!(category("x = 1\ny = x + 2\nprint(y)"), "code");
    assert_eq!(category("prepare()\nexecute()\ncleanup()"), "code");
}

#[test]
fn conservative_rules_keep_prose_as_text() {
    assert_eq!(category("请打开 https://example.com 查看文档"), "text");
    assert_eq!(
        category("Please run git status and send me the result."),
        "text"
    );
    assert_eq!(category("let me know when you are ready"), "text");
    assert_eq!(category("class action lawsuits can take years"), "text");
    assert_eq!(category("import this document"), "text");
    assert_eq!(category("use this approach"), "text");
    assert_eq!(category("from here import this document"), "text");
    assert_eq!(category("go home now"), "text");
    assert_eq!(category("make this easier"), "text");
    assert_eq!(category("Name = Alice\nCity = Beijing"), "text");
    assert_eq!(category("git"), "text");
    assert_eq!(category("true"), "text");
    assert_eq!(category("{this is ordinary text}"), "text");
    for text in [
        "user@example.com",
        "Please visit example.com today",
        "example.com second-value",
        "README.md",
        "main.rs",
        "config.toml",
        "archive.zip",
        "object.method",
        "com.example.App",
        "1.2.3",
        "v1.2.3",
        "example",
        "www.example",
        ".example.com",
        "example..com",
        "-foo.com",
        "foo-.com",
        "example.com:abc",
        "example.com:99999",
        r#"example.com\docs"#,
        "example.invalid",
    ] {
        assert_eq!(category(text), "text", "{text}");
    }
    assert_eq!(category(r#"C:\Tools\formatter.exe"#), "path");
    assert_eq!(category("/usr/bin/ls"), "path");
}

#[test]
fn only_scans_the_bounded_prefix() {
    let text = format!("{}\nconst value = 1;", "a".repeat(MAX_SAMPLE_BYTES));
    assert_eq!(category(&text), "text");

    let mut split_multibyte = "a".repeat(MAX_SAMPLE_BYTES - 1);
    split_multibyte.push('\u{4f60}');
    split_multibyte.push_str("\nconst value = 1;");
    assert_eq!(category(&split_multibyte), "text");

    let long_json = format!(r#"{{"value":"{}"}}"#, "a".repeat(MAX_SAMPLE_BYTES));
    assert_eq!(category(&long_json), "text");
}

#[test]
fn exposes_only_supported_categories() {
    for category in CATEGORIES {
        assert!(is_known_category(category));
    }
    assert!(!is_known_category("unknown"));
}
