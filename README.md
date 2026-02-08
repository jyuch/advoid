advoid
===

DNS-based Ad Blocker

## Overview

It blocks communication with ad networks by returning `NXDOMAIN` for DNS queries to domains or subdomains specified in the
definition file.

## Supported Platforms

Tested on Windows and Linux (Ubuntu). Should work on macOS as well.

## Usage

Prepare a definition file with domains that you want to block DNS queries, as shown below.

```
# comment line
# ignore blank line

example.com
```

Finding it difficult to prepare a definition file? Many websites that provide ad blocking software also publish blocklist files in a similar format.

| Argument                              | Environment Variable           | Description                                      |
|:--------------------------------------|:-------------------------------|:-------------------------------------------------|
| `--bind <BIND>`                       | -                              | Bind address                                     |
| `--upstream <UPSTREAM>`               | -                              | Upstream DNS resolver address                    |
| `--exporter <EXPORTER>`               | -                              | Prometheus exporter endpoint                     |
| `--block <BLOCK>`                     | -                              | Path to the definition file or URL               |
| `--otel <OTEL>`                       | -                              | OTel endpoint (optional)                         |
| `--sink <SINK>`                       | -                              | Event sink type (s3 or databricks, optional)     |
| `--s3-bucket <BUCKET>`                | -                              | S3 bucket name (required when --sink s3)         |
| `--s3-prefix <PREFIX>`                | -                              | S3 key prefix (optional)                         |
| `--databricks-host <HOST>`            | `DATABRICKS_HOST`              | Databricks workspace URL                         |
| `--databricks-client-id <ID>`         | `DATABRICKS_CLIENT_ID`         | Service principal client ID                      |
| `--databricks-client-secret <SECRET>` | `DATABRICKS_CLIENT_SECRET`     | Service principal client secret                  |
| `--databricks-volume-path <PATH>`     | `DATABRICKS_VOLUME_PATH`       | Volume path for event storage                    |
| `--sink-interval <SECONDS>`           | -                              | Batch upload interval in seconds (default: 1)    |
| `--sink-batch-size <SIZE>`            | -                              | Batch size for event uploads (default: 1000)     |

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
