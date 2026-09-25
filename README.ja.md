# gcal

[English](README.md) | 日本語

Rust で書いた、ターミナル用の軽量 Google カレンダークライアントです。

- **複数アカウント対応**：全アカウントの予定をまとめて1つの一覧で見られる
- **CLI と TUI の両方**：サブコマンドでスクリプトから使え、引数なしの `gcal` で対話型の画面が開く
- **OAuth が簡単**：`gcal login <名前>` でブラウザが開き、「許可」を押すだけ。コードのコピペは不要
- **小さい**：約 2MB のバイナリ1本。実行時に必要なライブラリはない（TLS も内蔵）

## インストール

```sh
cargo install --git https://github.com/polidog/gcal
```

## 初期設定

Google の仕様で、アプリごとに自分用の OAuth クライアントが必要です。最初に1回だけ作ります（10分くらい）。

1. Google Cloud Console で[プロジェクトを作成](https://console.cloud.google.com/projectcreate)します（名前は何でも可）。
2. [Google Calendar API を有効化](https://console.cloud.google.com/apis/library/calendar-json.googleapis.com)します。
3. メニューの「**Google Auth Platform**」を開きます。
   - 「**ブランディング**」：アプリ名と自分のメールアドレスを入力
   - 「**対象**」：「外部」を選び、使いたい Google アカウントを全部「**テストユーザー**」に追加
4. 「**クライアント**」→「**クライアントを作成**」で種類「**デスクトップアプリ**」を選び、JSON をダウンロードします。
5. 登録してログインします。

```sh
gcal init ~/Downloads/client_secret_xxx.json
gcal login work
gcal login private   # アカウントの数だけ繰り返す
```

`gcal init` は、クライアント ID とシークレットを `~/.config/gcal/client.json` にコピーするだけです。ダウンロードした JSON はそのあと消して構いません。

> 「対象」が「**テスト**」のままだと、ログインが7日で切れます。「**アプリを公開**」を押すと切れなくなります。「未確認のアプリ」という警告は出ますが、自分で使う分には問題ありません。

## 使い方

### CLI

```sh
gcal list                      # 今日から7日分、全アカウント
gcal list -d 14 -a work        # 14日分、1アカウントだけ
gcal list --all                # 不参加と返事した予定も表示
gcal list --ids                # 予定 ID も表示（編集・削除で使う）

gcal add 打ち合わせ "2026-09-26 10:00" "2026-09-26 11:00" -a work
gcal add 休み 2026-09-28 2026-09-29      # 終日（終了日も含む）

gcal edit <ID> --start "2026-09-26 14:00" --end "2026-09-26 15:00" -a work
gcal edit <ID> --title 新しいタイトル -a work
gcal delete <ID> -a work

gcal accounts                  # アカウント一覧
gcal logout work               # アカウントを削除
```

アカウントが1つだけなら `-a` は省略できます。

### TUI

`gcal`（または `gcal tui`）で起動します。

| キー | 動作 |
|---|---|
| `j` / `k` | 移動 |
| `h` / `l` | 前の週 / 次の週 |
| `t` | 今日に戻る |
| `Tab` | アカウント切り替え（全部 → 1つずつ） |
| `a` | 予定を追加（表示中のアカウントに） |
| `e` | 予定を編集 |
| `d` | 予定を削除（`y` で確定） |
| `x` | 不参加の予定の表示・非表示 |
| `o` | ブラウザで開く |
| `r` | 再読み込み |
| `q` | 終了 |

追加・編集フォームでは、`Tab` / `↑↓` で項目を移動し、`Enter` で保存、`Esc` でやめます。

## ファイル

| 場所 | 中身 |
|---|---|
| `~/.config/gcal/client.json` | OAuth クライアント（`gcal init` で作成） |
| `~/.config/gcal/accounts/<名前>.json` | アカウントごとのトークン（権限 0600） |

`$XDG_CONFIG_HOME` を設定していれば、そちらが使われます。

## 制限

- 表示するのは各アカウントの **primary** カレンダーだけです。
- 繰り返し予定の編集・削除は、その回の予定だけに反映されます。
