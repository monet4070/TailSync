# frozen_string_literal: true

require "yaml"

repository_root = File.expand_path("..", __dir__)
config_path = File.join(repository_root, ".github", "dependabot.yml")

def fail_policy(message)
  warn "Dependabot policy error: #{message}"
  exit 1
end

config = YAML.safe_load(
  File.read(config_path),
  permitted_classes: [],
  permitted_symbols: [],
  aliases: false
)

fail_policy("version must be 2") unless config["version"] == 2

updates = config["updates"]
fail_policy("updates must be an array") unless updates.is_a?(Array)

updates.each do |entry|
  ecosystem = entry["package-ecosystem"]

  if entry.key?("ignore")
    fail_policy("#{ecosystem} must not use ignore; it can suppress security updates")
  end

  if entry.key?("versioning-strategy")
    fail_policy(
      "#{ecosystem} must not set versioning-strategy without a security-impact review"
    )
  end
end

cargo = updates.find { |entry| entry["package-ecosystem"] == "cargo" }
fail_policy("cargo configuration is missing") unless cargo

expected_cargo_directories = [
  "/",
  "/windows/src-tauri",
  "/macos/src-tauri"
].sort

actual_cargo_directories = Array(cargo["directories"]).sort

unless actual_cargo_directories == expected_cargo_directories
  fail_policy("cargo must cover all three Cargo dependency graphs")
end

unless cargo["open-pull-requests-limit"] == 0
  fail_policy("ordinary Cargo version updates must remain disabled")
end

npm_entries = updates.select { |entry| entry["package-ecosystem"] == "npm" }
npm_directories = npm_entries.map { |entry| entry["directory"] }.sort

unless npm_directories == ["/site", "/windows"]
  fail_policy("npm must cover /windows and /site with separate entries")
end

expected_update_types = [
  "version-update:semver-minor",
  "version-update:semver-patch"
].sort

npm_entries.each do |entry|
  allow_rule = Array(entry["allow"]).find do |rule|
    rule["dependency-name"] == "*"
  end

  fail_policy("#{entry['directory']} must allow all dependency names") unless allow_rule

  actual_types = Array(allow_rule["update-types"]).sort

  unless actual_types == expected_update_types
    fail_policy("#{entry['directory']} must allow only patch/minor version updates")
  end
end

actions = updates.find do |entry|
  entry["package-ecosystem"] == "github-actions"
end

fail_policy("GitHub Actions configuration is missing") unless actions

actions_allow = Array(actions["allow"]).find do |rule|
  rule["dependency-name"] == "*"
end

fail_policy("GitHub Actions wildcard allow rule is missing") unless actions_allow

unless Array(actions_allow["update-types"]).sort == expected_update_types
  fail_policy("GitHub Actions must allow only patch/minor version updates")
end

puts "OK: Dependabot policy preserves unrestricted security updates."
