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

## Repeat Limiting (DialogVisible / GenemonVisible)

`DialogVisible` と `GenemonVisible` は条件継続中に繰り返し発火する。
無限通知を防ぐため `notification_max_repeat` で上限を設ける。

- 1 occurrence (出現〜消滅) の間に発火できる回数は `notification_max_repeat` まで。
- `DialogGone` / `GenemonGone` を受信すると `clear_condition` がカウンタをゼロにリセットし、
  次の occurrence では再び `max_repeat` 回まで発火できる。
- `notification_cooldown` が 0 より大きい場合、cooldown 期間中はさらに発火を抑制する。
- `notify_dialog_sustain` / `notify_round_sustain` / `notify_genemon_sustain` (sustain) が 0 より大きい場合、
  出現から sustain 経過後に初回発火する。
- `GenemonVisible` は `clear_condition` 時に `last_notified` を保持する (OCR ノイズ false-Gone で cooldown がリセットされるスパムを防止)。`DialogVisible` のみ `last_notified` を削除して次の出現を blocked しないようにする。

## RoundTrip Thresholds

RoundTrip 通知は探検/ガードでステージ別の閾値を持つ。

| Config Field             | Default | Description        |
| ------------------------ | ------- | ------------------ |
| `roundtrip_green`        | 60s     | 探検 Green 閾値    |
| `roundtrip_yellow`       | 120s    | 探検 Yellow 閾値   |
| `roundtrip_red`          | 180s    | 探検 Red 閾値      |
| `roundtrip_green_guard`  | 60s     | ガード Green 閾値  |
| `roundtrip_yellow_guard` | 120s    | ガード Yellow 閾値 |
| `roundtrip_red_guard`    | 180s    | ガード Red 閾値    |

`StageKind::Guard` の場合は Guard 閾値を使用。`Unknown` は Exploration 閾値に fallback。
UI では探検閾値の下にガード専用ブロックを縦配置 (既存ブロックのラベル変更なし)。

## Stage Kind Payload

`RoundVisible` イベント発火時、`DetectionEventPayload` に `stage_kind` フィールドを付与してフロントエンドへ送信する。

| `StageKind`   | `stage_kind` フィールド |
| ------------- | ----------------------- |
| `Exploration` | `Some("探検")`          |
| `Guard`       | `Some("ガード")`        |
| `Unknown`     | `None`                  |

`RoundGone` 等の他のイベントでは `stage_kind: None`。

フロントエンド (`ui/main.js`) は `stage_kind` を Round バッジのラベルとして使用する:

```js
case "RoundVisible": {
  const label = payload.stage_kind ?? "Visible";
  detectorState.round = { state: "ok", label, time: now };
  break;
}
```

`stage_kind` が `None` の場合(OCR 未実行 / `Unknown`)はフォールバックとして `"Visible"` を表示。

## Non-Goals

- Slack / LINE 等の第三チャンネル
- `NotificationChannels` enum によるリファクタリング
- 通知種別ごとの Desktop 有効化制御
