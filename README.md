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

| Argument                              | Environment Variable           | Description                                      |
|:--------------------------------------|:-------------------------------|:-------------------------------------------------|
| `--bind <BIND>`                       | -                              | Bind address                                     |
| `--upstream <UPSTREAM>`               | -                              | Upstream full resolver to forward DNS queries to |
| `--exporter <EXPORTER>`               | -                              | Prometheus exporter endpoint                     |
| `--block <BLOCK>`                     | -                              | Path to the definition file                      |
| `--otel <OTEL>`                       | -                              | OTel endpoint (optional)                         |
| `--databricks-host <HOST>`            | `DATABRICKS_HOST`              | Databricks workspace URL                         |
| `--databricks-client-id <ID>`         | `DATABRICKS_CLIENT_ID`         | Service principal client ID                      |
| `--databricks-client-secret <SECRET>` | `DATABRICKS_CLIENT_SECRET`     | Service principal client secret                  |
| `--databricks-volume-path <PATH>`     | `DATABRICKS_VOLUME_PATH`       | Volume path for event storage                    |

``` powershell
.\advoid.exe `
    --bind 192.168.2.32:53 `
    --upstream 1.1.1.1:53 `
    --exporter 192.168.2.32:3000 `
    --block 'C:\path\to\block\list\file.txt' `
    --otel http://localhost:4317
```

``` powershell
# Using environment variables (recommended for secrets)
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
