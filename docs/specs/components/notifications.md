# Notifications

DNA Assistant の通知チャンネル仕様。

## Channels

| Channel | Config Field      | Default | Description                     |
| ------- | ----------------- | ------- | ------------------------------- |
| Desktop | `desktop_enabled` | `true`  | Windows Toast notification      |
| Discord | `discord_enabled` | `false` | Discord Webhook (if configured) |

## Dispatch Logic

`discord_enabled` と `desktop_enabled` は独立して動作する。両方 `true` の場合は両チャンネルへ送信。

| discord_enabled | desktop_enabled | Result                 |
| --------------- | --------------- | ---------------------- |
| false           | false           | No notification        |
| false           | true            | Toast only             |
| true            | false           | Discord only           |
| true            | true            | Discord + Toast (both) |

## MonitorConfig Fields

```rust
/// Send notifications to Discord via webhook.
pub discord_enabled: bool,

/// Show Windows Toast desktop notifications.
#[serde(default = "default_true")]
pub desktop_enabled: bool,
```

`desktop_enabled` は `serde(default = "default_true")` で既存ユーザーの設定ファイルに
フィールドがない場合も `true` に fallback する。

## Implementation

- `send_notification_with_image()` — 通常の検知通知 (RoundGone, Dialog 等)
- `send_toast_and_discord()` — CaptureLost 専用、同じ独立ディスパッチ方式

## Non-Goals

- Slack / LINE 等の第三チャンネル
- `NotificationChannels` enum によるリファクタリング
- 通知種別ごとの Desktop 有効化制御
