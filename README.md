# DNA Assistant

![coverage](https://raw.githubusercontent.com/naa0yama/dna-assistant/badges/coverage.svg)
![test execution time](https://raw.githubusercontent.com/naa0yama/dna-assistant/badges/time.svg)

Duet Night Abyss のスクリーンモニター & Windows Toast 通知アプリ

## 概要

DNA Assistant は、Duet Night Abyss のゲーム画面を監視し、特定のイベントを検出して Windows Toast 通知で知らせるデスクトップアプリケーションです。Tauri v2 で構築されています。

## アーキテクチャ

マルチクレートワークスペース構成で、プラットフォームごとにクレートを分離しています。

| クレート        | パス                   | プラットフォーム       | 役割                                         |
| --------------- | ---------------------- | ---------------------- | -------------------------------------------- |
| `dna-detector`  | `crates/dna-detector/` | クロスプラットフォーム | 検出ロジック (ROI, 色判定, 検出器)           |
| `dna-capture`   | `crates/dna-capture/`  | Windows のみ           | スクリーンキャプチャ (WGC, PrintWindow), OCR |
| `dna-assistant` | `src-tauri/`           | Windows のみ           | Tauri v2 アプリ (IPC, 通知, トレイ)          |

フロントエンド: `ui/` — 静的 HTML + HTMX (CDN) + DaisyUI (CDN)、Node.js 不要。

データフロー: `dna-capture` → `image::RgbaImage` → `dna-detector` → `DetectionEvent` → `src-tauri` (通知/UI)

## 必要要件

- Docker
- Visual Studio Code + Dev Containers 拡張機能

> **Note:** Windows 専用クレート (`dna-capture`, `src-tauri`) は WSL2 / DevContainer 上ではビルドのみ可能です。実行・Tauri 開発には Windows ネイティブ環境が必要です。

## セットアップ

```bash
git clone https://github.com/naa0yama/dna-assistant.git
cd dna-assistant
```

VS Code のコマンドパレット (`Ctrl+Shift+P`) から「Dev Containers: Reopen in Container」を選択してください。

## 使い方

すべてのタスクは `mise run <task>` で実行します。

### 基本操作

```bash
mise run build              # デバッグビルド
mise run build:release      # リリースビルド (Tauri バンドル)
mise run test               # テスト実行 (ワークスペース全体)
mise run test:core          # テスト実行 (dna-detector のみ, DevContainer OK)
mise run test:watch         # TDD ウォッチモード
mise run test:doc           # ドキュメントテスト
```

### コード品質

```bash
mise run fmt                # フォーマット (cargo fmt + dprint)
mise run fmt:check          # フォーマットチェック
mise run clippy             # Lint
mise run clippy:strict      # Lint (warnings をエラー扱い)
mise run clippy:core        # Lint (dna-detector のみ, DevContainer OK)
mise run ast-grep           # ast-grep カスタムルールチェック
```

### コミット前チェック

```bash
mise run pre-commit         # clean:sweep + fmt:check + clippy:strict + ast-grep + lint:gh
```

### DevContainer / WSL2 で使えるコマンド

Windows 専用クレートに依存しないタスクです。

```bash
mise run check:core         # コンパイルチェック (dna-detector のみ)
mise run test:core          # テスト (dna-detector のみ)
mise run clippy:core        # Lint (dna-detector のみ)
mise run miri:core          # Miri (dna-detector のみ)
```

## プロジェクト構造

```
.
├── .cargo/                     # Cargo 設定
├── .devcontainer/              # Dev Container 設定
├── .githooks/                  # Git hooks (mise run 連携)
│   ├── commit-msg              # Conventional Commits 検証
│   ├── pre-commit              # コミット前チェック
│   └── pre-push                # プッシュ前チェック
├── .github/                    # GitHub Actions & 設定
│   ├── actions/                # カスタムアクション
│   ├── rulesets/               # Protection rulesets
│   └── workflows/              # CI/CD ワークフロー
├── .vscode/                    # VS Code 設定
├── ast-rules/                  # ast-grep プロジェクトルール
├── crates/
│   ├── dna-capture/            # スクリーンキャプチャ (Windows のみ)
│   └── dna-detector/           # 検出ロジック (クロスプラットフォーム)
├── docs/                       # ドキュメント
│   └── specs/                  # 設計仕様書
├── src-tauri/                  # Tauri v2 アプリ (Windows のみ)
├── ui/                         # フロントエンド (HTML + HTMX + DaisyUI)
├── Cargo.toml                  # ワークスペース設定と共有依存関係
├── deny.toml                   # cargo-deny 設定
├── Dockerfile                  # Docker イメージ定義
├── dprint.jsonc                # dprint フォーマッター設定
├── mise.toml                   # ツール管理 & タスクランナー
└── sgconfig.yml                # ast-grep 設定ファイル
```

## ライセンス

このプロジェクトは [AGPL-3.0](./LICENSE) ライセンスの下で公開されています。

### サードパーティライセンスについて

Dev Container の起動時に [OpenObserve Enterprise Edition](https://openobserve.ai/) が自動的にダウンロード・インストールされます。Enterprise 版は MCP (Model Context Protocol) サーバー機能など OSS 版にはない付加機能を備えているため採用しています。Enterprise 版は 200GB/Day のインジェストクォータ内であれば無料で利用できます。

OpenObserve Enterprise Edition は [EULA (End User License Agreement)](https://openobserve.ai/enterprise-license/) の下で提供されており、OSS 版 (AGPL-3.0) とはライセンスが異なります。Enterprise 版の機能一覧は [OpenObserve Enterprise](https://openobserve.ai/docs/features/enterprise/) を参照してください。
