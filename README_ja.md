advoid
===

DNSベースのアドブロッカー

## 概要

定義ファイルのドメインもしくはサブドメインのDNS問い合わせに対して`NXDOMAIN`を返すことによりアドネットワークへの通信を遮断します。

## 対応プラットフォーム

WindowsとLinux（Ubuntu）で動くことは確認しています。

たぶんMacでも動くと思います。

## 使い方

以下のようなDNS問い合わせをブロックしたいドメインを定義した定義ファイルを用意します。

```
# comment line
# ignore blank line

example.com
```

定義ファイルを用意するのが大変？
そういえばどこかのアドブロッカーアプリを公開しているサイトがこのフォーマットによく似た定義ファイルを公開してくれていますね。

| 引数                                  | 環境変数                         | 説明                                           |
|:-------------------------------------|:-------------------------------|:---------------------------------------------|
| `--bind <BIND>`                      | -                              | バインドアドレス                                   |
| `--upstream <UPSTREAM>`              | -                              | DNS問い合わせを転送する上位のフルリゾルバ                   |
| `--exporter <EXPORTER>`              | -                              | Prometheus エンドポイント                         |
| `--block <BLOCK>`                    | -                              | 定義ファイルのパスまたはURL                           |
| `--otel <OTEL>`                      | -                              | OTelエンドポイント（オプション）                        |
| `--sink <SINK>`                      | -                              | イベントシンクタイプ (s3 または databricks、オプション)     |
| `--s3-bucket <BUCKET>`               | -                              | S3バケット名 (--sink s3 の場合必須)                 |
| `--s3-prefix <PREFIX>`               | -                              | S3キープレフィックス（オプション）                        |
| `--databricks-host <HOST>`           | `DATABRICKS_HOST`              | Databricksワークスペース URL                     |
| `--databricks-client-id <ID>`        | `DATABRICKS_CLIENT_ID`         | サービスプリンシパルのクライアントID                       |
| `--databricks-client-secret <SECRET>`| `DATABRICKS_CLIENT_SECRET`     | サービスプリンシパルのクライアントシークレット                  |
| `--databricks-volume-path <PATH>`    | `DATABRICKS_VOLUME_PATH`       | イベント保存用のボリュームパス                          |
| `--sink-interval <SECONDS>`          | -                              | バッチアップロード間隔（秒）（デフォルト: 1）                |
| `--sink-batch-size <SIZE>`           | -                              | イベントアップロードのバッチサイズ（デフォルト: 1000）          |

``` powershell
.\advoid.exe `
    --bind 192.168.2.32:53 `
    --upstream 1.1.1.1:53 `
    --exporter 192.168.2.32:3000 `
    --block 'C:\path\to\block\list\file.txt' `
    --otel http://localhost:4317
```

``` powershell
$env:DATABRICKS_HOST="https://workspace.cloud.databricks.com"
$env:DATABRICKS_CLIENT_ID="<client-id>"
$env:DATABRICKS_CLIENT_SECRET="<secret>"
$env:DATABRICKS_VOLUME_PATH="/Volumes/catalog/schema/volume"

.\advoid.exe `
    --bind 192.168.2.32:53 `
    --upstream 1.1.1.1:53 `
    --exporter 192.168.2.32:3000 `
    --block 'C:\path\to\block\list\file.txt' `
    --sink databricks
```

``` powershell
# $env:AWS_ENDPOINT_URL='http://192.168.1.1:8080'
$env:AWS_REGION='us-west-2'
$env:AWS_ACCESS_KEY_ID='<access-key>'
$env:AWS_SECRET_ACCESS_KEY='<secret>'

.\advoid.exe `
    --bind 192.168.2.32:53 `
    --upstream 1.1.1.1:53 `
    --exporter 192.168.2.32:3000 `
    --block 'C:\path\to\block\list\file.txt' `
    --sink s3 `
    --s3-bucket my-logs-bucket `
    --s3-prefix dns-events
```
