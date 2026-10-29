# Proa documentation Go SDK

Official, dependency-free Go client for the public [Proa documentation API](https://proa.so/docs/api).
Search and read Rust framework documentation without credentials. This package
does not scaffold or build applications; use the Proa Rust CLI for that.

Requires Go 1.23 or newer. Install the module with:

```sh
go get github.com/Proa-Labs/proa/sdk/go@v0.0.0-20261029004544-c75fce01a7f4
```

```go
package main

import (
    "context"
    "fmt"
    "log"

    proadocs "github.com/Proa-Labs/proa/sdk/go"
)

func main() {
    client, err := proadocs.NewClient(proadocs.Options{})
    if err != nil { log.Fatal(err) }
    results, err := client.Search(context.Background(), "streaming", proadocs.PageOptions{Limit: 5})
    if err != nil { log.Fatal(err) }
    for _, doc := range results.Results {
        fmt.Println(doc.Title, doc.Path)
    }
}
```

`Search`, `List`, and `Get` return typed responses. `WalkDocuments` follows
pagination and stops on repeated cursors, context cancellation, or a visitor
error. Every request has a 15-second default timeout; `Options.Timeout` and a
caller context can shorten it. A custom `HTTPClient` supports test transports.
Responses are limited to 16 MiB. The client never retries automatically.

Inspect HTTP failures with `errors.As(err, &apiError)` using a
`*proadocs.APIError`. It preserves `StatusCode`, the optional `Problem`, and
`RetryAfter`. Honor that delay for 429 responses. Do not send private data or
secrets as documentation search terms.

For fixture-based integration tests, set `Options.BaseURL` to
`https://proa.so/sandbox`; the base path is preserved for every API request.
Local servers can use `http://localhost:5555`.

Run `go test -race ./...` and `go vet ./...` in this directory. The tests use
local HTTP servers and require no credentials or live API access.
