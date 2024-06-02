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

| 引数                      | 説明                     |
|:------------------------|:-----------------------|
| `--bind <BIND>`         | バインドアドレス               |
| `--upstream <UPSTREAM>` | DNS問い合わせを転送する上位のフルリゾルバ |
| `--exporter <EXPORTER>` | Prometheus エンドポイント     |
| `--block <BLOCK>`       | 定義ファイルのパス              |
| `--otel <OTEL>`         | OTelエンドポイント（オプション）     |

``` powershell
.\advoid.exe `
    --bind 192.168.2.32:53 `
    --upstream 1.1.1.1:53 `
    --console 192.168.2.32:3000 `
    --block 'C:\path\to\block\list\file.txt' `
    --otel http://localhost:4317
```
