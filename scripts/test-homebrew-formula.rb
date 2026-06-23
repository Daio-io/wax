#!/usr/bin/env ruby
# frozen_string_literal: true

formula_path = File.expand_path("../homebrew/Formula/wax.rb", __dir__)
formula = File.read(formula_path)

def assert(condition, message)
  return if condition

  warn "FAIL: #{message}"
  exit 1
end

RubyVM::InstructionSequence.compile_file(formula_path)

assert(
  formula.include?('require "tmpdir"'),
  "formula should require tmpdir for a neutral refresh repo root"
)
assert(
  formula.match?(/def\s+post_install\b/),
  "formula should define post_install so brew install/upgrade refreshes language packs"
)
assert(
  formula.include?('Dir.mktmpdir("wax-language-refresh")'),
  "post_install should create a neutral temporary repo root"
)
assert(
  formula.match?(/quiet_system\s+bin\/"wax",\s*"language",\s*"update",\s*"--all",\s*"--repo-root",\s*repo_root/),
  "post_install should run wax language update --all with --repo-root"
)
assert(
  formula.include?("Unable to refresh installed wax language packs after install"),
  "post_install should warn when language-pack refresh cannot complete"
)
