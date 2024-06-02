advoid
===

DNS-based Ad Blocker

## Overview

It blocks communication to ad networks by returning `NXDOMAIN` for DNS queries to domains or subdomains specified in the
definition file.

## Supported Platforms

It has been confirmed to work on Windows and Linux (Ubuntu).

It will probably work on Mac as well.

## Usage

Prepare a definition file with domains for which you want to block DNS queries, as shown below.

```
# comment line
# ignore blank line

example.com
```

Finding it difficult to prepare a definition file?
By the way, some websites that publish ad blocker apps also provide definition files in a similar format.

| Argument                | Description                                      |
|:------------------------|:-------------------------------------------------|
| `--bind <BIND>`         | Bind address                                     |
| `--upstream <UPSTREAM>` | Upstream full resolver to forward DNS queries to |
| `--exporter <EXPORTER>` | Prometheus exporter endpoint                     |
| `--block <BLOCK>`       | Path to the definition file                      |
| `--otel <OTEL>`         | OTel endpoint (optional)                         |

``` powershell
.\advoid.exe `
    --bind 192.168.2.32:53 `
    --upstream 1.1.1.1:53 `
    --console 192.168.2.32:3000 `
    --block 'C:\path\to\block\list\file.txt' `
    --otel http://localhost:4317
```
