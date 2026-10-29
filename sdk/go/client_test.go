// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

package proadocs

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func testClient(t *testing.T, handler http.HandlerFunc) *Client {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	client, err := NewClient(Options{BaseURL: server.URL + "/sandbox", HTTPClient: server.Client()})
	if err != nil {
		t.Fatal(err)
	}
	return client
}

func TestEncodedRequestsAndTypedResponses(t *testing.T) {
	client := testClient(t, func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.Header.Get("Accept") != "application/json" || r.Header.Get("Authorization") != "" {
			t.Errorf("unexpected request: %s %v", r.Method, r.Header)
		}
		switch r.URL.Path {
		case "/sandbox/api/v1/search":
			if r.URL.Query().Get("q") != "Rust & café?" || r.URL.Query().Get("limit") != "2" || r.URL.Query().Get("cursor") != "a+b/=" {
				t.Errorf("incorrect encoded query: %v", r.URL.Query())
			}
			fmt.Fprint(w, `{"results":[{"title":"Result","path":"/docs/a","markdown_url":"/docs/a.md","description":"Example"}],"total":3,"next_cursor":"2"}`)
		case "/sandbox/api/v1/docs/page":
			if r.URL.Query().Get("path") != "/docs/a?b&c" {
				t.Errorf("incorrect document path: %v", r.URL.Query())
			}
			fmt.Fprint(w, `{"title":"Page","path":"/docs/a","markdown_url":"/docs/a.md","description":"Example","markdown":"# Page\n"}`)
		default:
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
	})
	cursor := "a+b/="
	page, err := client.Search(context.Background(), "Rust & café?", PageOptions{Limit: 2, Cursor: &cursor})
	if err != nil || len(page.Results) != 1 || page.Total != 3 || page.NextCursor == nil || *page.NextCursor != "2" {
		t.Fatalf("search: %#v, %v", page, err)
	}
	doc, err := client.Get(context.Background(), "/docs/a?b&c")
	if err != nil || doc.Title != "Page" || doc.Markdown != "# Page\n" {
		t.Fatalf("document: %#v, %v", doc, err)
	}
}

func TestPaginationAndEarlyStop(t *testing.T) {
	var requests atomic.Int32
	client := testClient(t, func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		if r.URL.Query().Get("limit") != "50" {
			t.Error("missing page limit")
		}
		if r.URL.Query().Get("cursor") == "" {
			fmt.Fprint(w, `{"results":[{"path":"/first"}],"total":2,"next_cursor":"1"}`)
		} else {
			fmt.Fprint(w, `{"results":[{"path":"/second"}],"total":2,"next_cursor":null}`)
		}
	})
	var paths []string
	err := client.WalkDocuments(context.Background(), func(d DocumentSummary) error { paths = append(paths, d.Path); return nil })
	if err != nil || strings.Join(paths, ",") != "/first,/second" || requests.Load() != 2 {
		t.Fatalf("pagination: %v, %v, requests=%d", paths, err, requests.Load())
	}
	stop := errors.New("stop")
	err = client.WalkDocuments(context.Background(), func(DocumentSummary) error { return stop })
	if !errors.Is(err, stop) || requests.Load() != 3 {
		t.Fatalf("visitor cancellation: %v, requests=%d", err, requests.Load())
	}
}

func TestRepeatingCursorIsRejected(t *testing.T) {
	var requests atomic.Int32
	client := testClient(t, func(w http.ResponseWriter, _ *http.Request) {
		requests.Add(1)
		fmt.Fprint(w, `{"results":[],"total":1,"next_cursor":"same"}`)
	})
	err := client.WalkDocuments(context.Background(), func(DocumentSummary) error { return nil })
	if err == nil || requests.Load() != 2 {
		t.Fatalf("repeating cursor: %v, requests=%d", err, requests.Load())
	}
}

func TestStructuredFailuresAndNoRetries(t *testing.T) {
	var requests atomic.Int32
	client := testClient(t, func(w http.ResponseWriter, _ *http.Request) {
		requests.Add(1)
		w.Header().Set("Retry-After", "17")
		w.WriteHeader(http.StatusTooManyRequests)
		fmt.Fprint(w, `{"type":"about:blank","status":429,"code":"rate_limited","detail":"Wait before retrying."}`)
	})
	_, err := client.List(context.Background(), PageOptions{})
	var apiError *APIError
	if !errors.As(err, &apiError) || apiError.StatusCode != 429 || apiError.Problem == nil || apiError.Problem.Code != "rate_limited" || apiError.RetryAfter == nil || *apiError.RetryAfter != 17*time.Second || requests.Load() != 1 {
		t.Fatalf("API error: %#v, %v", apiError, err)
	}
}

func TestRequestDeadline(t *testing.T) {
	client := testClient(t, func(_ http.ResponseWriter, r *http.Request) { <-r.Context().Done() })
	client.timeout = 30 * time.Millisecond
	_, err := client.List(context.Background(), PageOptions{})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected deadline, got %v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err = client.List(ctx, PageOptions{})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("expected cancellation, got %v", err)
	}
}

func TestMalformedAndOversizedResponses(t *testing.T) {
	for _, body := range []string{"{", "{} {}", strings.Repeat(" ", maxResponseBytes+1)} {
		t.Run(fmt.Sprintf("bytes_%d", len(body)), func(t *testing.T) {
			client := testClient(t, func(w http.ResponseWriter, _ *http.Request) { fmt.Fprint(w, body) })
			if _, err := client.List(context.Background(), PageOptions{}); err == nil {
				t.Fatal("invalid response accepted")
			}
		})
	}
}

func TestInvalidConfiguration(t *testing.T) {
	for _, base := range []string{"file:///tmp/docs", "https://user:password@example.com", "https://example.com?q=x", "https://example.com#part", "https:///missing-host"} {
		if _, err := NewClient(Options{BaseURL: base}); err == nil {
			t.Errorf("accepted %q", base)
		}
	}
	if _, err := NewClient(Options{Timeout: -time.Second}); err == nil {
		t.Error("accepted negative timeout")
	}
}
