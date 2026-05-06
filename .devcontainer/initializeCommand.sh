#!/usr/bin/env bash
set -euo pipefail

# dirs from mounts
mkdir -p \
	~/.claude/ \
	~/.config/gh \
	~/.gitconfig.d

# files from mounts
touch \
	~/.claude.json \
	~/.claude/.config.json \
	~/.gitconfig

# graft:keep-start
# Project-specific dependencies are listed here.

# graft:keep-end
