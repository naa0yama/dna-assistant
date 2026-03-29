# アプリケーションレイヤー (src-tauri)

> 親ドキュメント: [detection-overview.md](./detection-overview.md)
>
> 関連ドキュメント:
>
> - [Capture レイヤー](./capture.md)
> - [RoundDetector](./round-detector.md)
> - [SkillDetector](./skill-detector.md)

## 1.1 背景

DNA Assistant のアプリケーションレイヤー(`src-tauri`)は、`dna-capture` と `dna-detector` を統合し、ゲーム画面の監視 → 検出 → 通知の一連のパイプラインを駆動する。Tauri v2 デスクトップアプリとして、IPC コマンド経由でフロントエンドと通信し、Windows Toast 通知でユーザーにアラートを送信する。

問題点:

- キャプチャ(2 秒間隔)と検出処理は UI スレッドをブロックしてはならない
- 複数の Detector が異なるタイミングでイベントを発生させ、通知の重複・氾濫を防ぐ必要がある
- モニタリング状態(起動中/停止中/エラー)をフロントエンドにリアルタイムで反映する必要がある

目標:

バックグラウンドスレッドでキャプチャ → 検出 → 通知ループを駆動し、Tauri IPC + イベントシステムでフロントエンドと状態を同期する。

## 1.2 モジュール構成

| モジュール     | ファイル          | 責務                                                     |
| -------------- | ----------------- | -------------------------------------------------------- |
| `commands`     | `commands.rs`     | IPC コマンドハンドラ(start/stop/status/preview/settings) |
| `monitor`      | `monitor.rs`      | キャプチャ → 検出 → 通知のバックグラウンドループ         |
| `notification` | `notification.rs` | Toast 通知の送信・重複制御                               |
| `settings`     | `settings.rs`     | 設定の永続化(JSON ファイルへの読み書き)                  |

## 1.3 モニターループ

### 状態遷移

```mermaid
statechart-v2
    [*] --> Idle
    Idle --> SearchingWindow: start_monitoring
    SearchingWindow --> Capturing: ウィンドウ発見
    SearchingWindow --> Idle: stop_monitoring
    Capturing --> SearchingWindow: ウィンドウ消失
    Capturing --> Idle: stop_monitoring
```

### ループ処理フロー

```mermaid
flowchart TD
    START([ループ開始]) --> FIND["window::find_game()\nゲームウィンドウ検索"]
    FIND --> FOUND{"発見?"}
    FOUND -- No --> WAIT_FIND["3 秒待機 → 再検索"]
    WAIT_FIND --> STOP_CHECK_1{"stop 要求?"}
    STOP_CHECK_1 -- Yes --> EXIT([ループ終了])
    STOP_CHECK_1 -- No --> FIND
    FOUND -- Yes --> BACKEND["バックエンド初期化\n(WGC → PrintWindow フォールバック)"]
    BACKEND --> CAPTURE["capture_frame()"]
    CAPTURE --> SUCCESS{"取得成功?"}
    SUCCESS -- No --> RETRY{"リトライ上限?"}
    RETRY -- No --> CAPTURE
    RETRY -- Yes --> FIND
    SUCCESS -- Yes --> TITLEBAR["crop_titlebar()"]
    TITLEBAR --> DETECT["各 Detector.analyze()"]
    DETECT --> DEBOUNCE["DebouncedDetector\nフィルタリング"]
    DEBOUNCE --> NOTIFY["通知判定"]
    NOTIFY --> EMIT["Tauri イベント送信\n(フロントエンドへ)"]
    EMIT --> INTERVAL["キャプチャ間隔待機\n(デフォルト 2 秒)"]
    INTERVAL --> ALIVE{"ウィンドウ存在?"}
    ALIVE -- Yes --> STOP_CHECK_2{"stop 要求?"}
    ALIVE -- No --> FIND
    STOP_CHECK_2 -- Yes --> EXIT
    STOP_CHECK_2 -- No --> CAPTURE
```

### スレッドモデル

| スレッド         | 役割                                          |
| ---------------- | --------------------------------------------- |
| メインスレッド   | Tauri イベントループ、IPC コマンドハンドラ    |
| モニタースレッド | キャプチャ → 検出 → 通知ループ(`std::thread`) |

モニタースレッドの制御は `Arc<AtomicBool>` の停止フラグで行う。`start_monitoring` でスレッドを起動し、`stop_monitoring` でフラグを立ててスレッド終了を待機する。

## 1.4 IPC コマンド

### `start_monitoring`

モニターループを開始する。既に起動中の場合はエラーを返す。

```rust
#[tauri::command]
async fn start_monitoring(app_handle: AppHandle, state: State<'_, MonitorState>) -> Result<(), String>;
```

### `stop_monitoring`

モニターループを停止する。未起動の場合は何もしない。

```rust
#[tauri::command]
async fn stop_monitoring(app_handle: AppHandle, state: State<'_, MonitorState>) -> Result<(), String>;
```

### `get_status`

現在のモニタリング状態を返す。

```rust
#[tauri::command]
fn get_status(state: State<'_, MonitorState>) -> MonitorStatus;
```

```rust
#[derive(Debug, Clone, Serialize)]
pub struct MonitorStatus {
    /// Current monitoring state.
    pub state: MonitoringState,
    /// Total frames captured since last start.
    pub frames_captured: u64,
    /// Total detection events since last start.
    pub events_detected: u64,
    /// Last detection event summary (if any).
    pub last_event: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum MonitoringState {
    Idle,
    SearchingWindow,
    Capturing,
}
```

### `get_capture_preview`

最新のキャプチャフレームを base64 エンコード PNG + メタデータとして返す。プレビュー用に最大幅 640px にダウンスケールする。

```rust
#[tauri::command]
fn get_capture_preview(state: State<'_, MonitorState>) -> CapturePreview;
```

```rust
#[derive(Debug, Clone, Serialize)]
pub struct CapturePreview {
    /// Base64-encoded PNG image data.
    pub image_base64: Option<String>,
    /// Capture metadata.
    pub info: CaptureInfo,
}
```

### `get_settings`

現在のモニター設定を返す。

```rust
#[tauri::command]
fn get_settings(state: State<'_, MonitorState>) -> MonitorConfig;
```

### `save_settings`

モニター設定を更新し、ディスクに永続化する。

```rust
#[tauri::command]
async fn save_settings(app_handle: AppHandle, state: State<'_, MonitorState>, config: MonitorConfig) -> Result<(), String>;
```

### `greet`(既存)

接続テスト用コマンド。

## 1.5 Tauri イベント

バックエンドからフロントエンドへの状態通知には Tauri のイベントシステムを使用する。

| イベント名        | ペイロード                         | タイミング         |
| ----------------- | ---------------------------------- | ------------------ |
| `monitor-status`  | `MonitorStatus` (JSON)             | 状態変化時         |
| `detection-event` | `{ kind: string, detail: string }` | 検出イベント発生時 |

フロントエンドは `listen()` でイベントを購読し、リアルタイムに UI を更新する。

## 1.6 通知判定ロジック

### 通知トリガー条件

detection-overview.md セクション 1.6 で定義されたトリガーを実装する。

| トリガー         | 条件                   | 持続時間 | 優先度 | 通知タイトル       |
| ---------------- | ---------------------- | -------- | ------ | ------------------ |
| Q スキル SP 枯渇 | `SkillGreyed` が持続   | 5 秒     | 高     | "Q スキル SP 枯渇" |
| ダイアログ表示   | `DialogVisible` が持続 | 3 秒     | 高     | "ダイアログ検出"   |
| ラウンド完了     | `RoundGone` が持続     | 5 秒     | 中     | "ラウンド完了"     |
| 味方 HP 低下     | `AllyHpLow` が持続     | 10 秒    | 低     | "味方 HP 低下"     |

### 通知重複制御

同一トリガーの通知は **60 秒間** 再送信しない(cooldown)。これによりトースト通知の氾濫を防ぐ。

### 持続時間判定

`DebouncedDetector` のクールダウンは偽陽性フィルタリング(ミリ秒〜数秒単位)であるのに対し、通知の持続時間判定はユーザーへの通知価値判断(秒単位)である。

```
Detector → DebouncedDetector(偽陽性除去) → NotificationManager(持続時間判定) → Toast
```

`NotificationManager` は各トリガーの最新イベント時刻を保持し、持続時間を超えた場合のみ通知を送信する。

## 1.7 フロントエンド UI

### 画面構成

420x640 ピクセルのシングルウィンドウ。DaisyUI (dark theme) を使用。

```
┌────────────────────────────┐
│  DNA Assistant             │
├────────────────────────────┤
│  [Status Card]             │
│  状態: ○ Idle / Capturing  │
│  Frames: 0   Events: 0    │
├────────────────────────────┤
│  [Control Card]            │
│  [ Start Monitoring ]      │
│  [ Stop Monitoring  ]      │
├────────────────────────────┤
│  [Event Log Card]          │
│  12:34:56 SkillGreyed      │
│  12:34:50 RoundVisible     │
│  12:34:48 DialogGone       │
│  ...                       │
└────────────────────────────┘
```

### コンポーネント

| コンポーネント | 内容                                                   |
| -------------- | ------------------------------------------------------ |
| Status Card    | 現在の状態バッジ、フレーム数、イベント数               |
| Control Card   | Start / Stop ボタン(状態に応じて有効/無効を切り替え)   |
| Event Log Card | 直近の検出イベントを時系列で表示(最大 50 件、新しい順) |

### IPC 連携

- ボタンクリック時: `invoke("start_monitoring")` / `invoke("stop_monitoring")`
- 初回ロード時: `invoke("get_status")` でステータス取得
- リアルタイム更新: `listen("monitor-status")` / `listen("detection-event")`

## 1.8 Tauri 状態管理

```rust
pub struct MonitorState {
    /// Monitor thread handle + stop flag.
    inner: Mutex<Option<MonitorHandle>>,
    /// Shared status for IPC queries.
    status: Arc<Mutex<MonitorStatus>>,
}

struct MonitorHandle {
    stop_flag: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}
```

`MonitorState` は `tauri::Builder::manage()` で登録し、各コマンドで `State<'_, MonitorState>` として受け取る。

## 1.9 エラーハンドリング

| 状況                       | 挙動                                             |
| -------------------------- | ------------------------------------------------ |
| ゲームウィンドウ未検出     | `SearchingWindow` 状態で 3 秒間隔で再検索        |
| キャプチャバックエンド失敗 | WGC → PrintWindow フォールバック後、再検索に移行 |
| 検出処理例外               | ログ出力、次フレームで継続                       |
| 通知送信失敗               | ログ出力、次のトリガーで再試行                   |
| モニター二重起動           | `start_monitoring` がエラーを返す                |

## 1.10 検討事項

- [ ] システムトレイアイコン — 最小化時にトレイに格納、状態をアイコンで表示
- [ ] キャプチャ間隔のユーザー設定 UI — 現在はデフォルト 2 秒固定
- [ ] 通知音のカスタマイズ — Windows Toast のオーディオ設定
- [ ] 検出結果のスクリーンショット保存 — デバッグ・検証用
- [ ] Phase 2 OCR 検出との統合 — `dna-capture` の OCR モジュール実装後
