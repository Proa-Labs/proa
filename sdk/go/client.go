// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Package proadocs is the official Go client for Proa's public documentation API.
//
// Search and read Rust framework documentation at https://proa.so/docs/api.
// No credentials are required. This client does not build or deploy applications.
package proadocs

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

const DefaultBaseURL = "https://proa.so"
const maxResponseBytes = 16 * 1024 * 1024

// Options configures a client. Zero values select production, a 15-second
// per-request timeout, and the default HTTP transport. A base URL can include
// a path prefix, for example https://proa.so/sandbox.
type Options struct {
	BaseURL    string
	Timeout    time.Duration
	HTTPClient *http.Client
}

// Client is safe for concurrent use when its supplied HTTPClient is too.
type Client struct {
	baseURL *url.URL
	timeout time.Duration
	http    *http.Client
}

func NewClient(options Options) (*Client, error) {
	if options.BaseURL == "" {
		options.BaseURL = DefaultBaseURL
	}
	base, err := url.Parse(options.BaseURL)
	if err != nil {
		return nil, fmt.Errorf("invalid Proa base URL: %w", err)
	}
	if (base.Scheme != "http" && base.Scheme != "https") || base.Hostname() == "" || base.User != nil || base.RawQuery != "" || base.ForceQuery || base.Fragment != "" || base.Opaque != "" {
		return nil, errors.New("Proa base URL must be HTTP(S) without credentials, query, or fragment")
	}
	if options.Timeout < 0 {
		return nil, errors.New("Proa timeout must be positive")
	}
	if options.Timeout == 0 {
		options.Timeout = 15 * time.Second
	}
	if options.HTTPClient == nil {
		options.HTTPClient = &http.Client{}
	}
	base.Path = strings.TrimRight(base.Path, "/") + "/"
	base.RawPath = ""
	return &Client{baseURL: base, timeout: options.Timeout, http: options.HTTPClient}, nil
}

type DocumentSummary struct {
	Title       string `json:"title"`
	Path        string `json:"path"`
	MarkdownURL string `json:"markdown_url"`
	Description string `json:"description"`
}

type Document struct {
	DocumentSummary
	Markdown string `json:"markdown"`
}

type DocumentList struct {
	Results    []DocumentSummary `json:"results"`
	Total      int               `json:"total"`
	NextCursor *string           `json:"next_cursor"`
}

// PageOptions controls pagination. Limit zero uses the server default (10).
// Reuse a returned NextCursor only with the same query and deployment.
type PageOptions struct {
	Limit  int
	Cursor *string
}

type Problem struct {
	Type          string `json:"type"`
	Title         string `json:"title"`
	Status        int    `json:"status"`
	Code          string `json:"code"`
	Detail        string `json:"detail"`
	Documentation string `json:"documentation"`
}

// APIError preserves the HTTP status, optional problem document, and retry delay.
// The client never retries automatically. Use errors.As to inspect this error.
type APIError struct {
	StatusCode int
	Problem    *Problem
	RetryAfter *time.Duration
}

func (e *APIError) Error() string {
	if e.Problem != nil && e.Problem.Detail != "" {
		return fmt.Sprintf("Proa API HTTP %d: %s", e.StatusCode, e.Problem.Detail)
	}
	return fmt.Sprintf("Proa API HTTP %d", e.StatusCode)
}

func (c *Client) Search(ctx context.Context, query string, options PageOptions) (*DocumentList, error) {
	params := pageParameters(options)
	params.Set("q", query)
	var result DocumentList
	err := c.request(ctx, "api/v1/search", params, &result)
	return &result, err
}

func (c *Client) List(ctx context.Context, options PageOptions) (*DocumentList, error) {
	var result DocumentList
	err := c.request(ctx, "api/v1/docs", pageParameters(options), &result)
	return &result, err
}

func (c *Client) Get(ctx context.Context, path string) (*Document, error) {
	var result Document
	err := c.request(ctx, "api/v1/docs/page", url.Values{"path": {path}}, &result)
	return &result, err
}

// WalkDocuments visits every public document, following pagination with batches
// of 50. Return an error from visit or cancel ctx to stop. Repeated or empty
// continuation cursors are rejected rather than looping indefinitely.
func (c *Client) WalkDocuments(ctx context.Context, visit func(DocumentSummary) error) error {
	if visit == nil {
		return errors.New("Proa document visitor must not be nil")
	}
	options := PageOptions{Limit: 50}
	seen := make(map[string]bool)
	for {
		page, err := c.List(ctx, options)
		if err != nil {
			return err
		}
		for _, document := range page.Results {
			if err := ctx.Err(); err != nil {
				return err
			}
			if err := visit(document); err != nil {
				return err
			}
		}
		if page.NextCursor == nil {
			return nil
		}
		cursor := *page.NextCursor
		if cursor == "" || seen[cursor] {
			return errors.New("invalid or repeating Proa pagination cursor")
		}
		seen[cursor] = true
		options.Cursor = page.NextCursor
	}
}

func pageParameters(options PageOptions) url.Values {
	params := make(url.Values)
	if options.Limit != 0 {
		params.Set("limit", strconv.Itoa(options.Limit))
	}
	if options.Cursor != nil {
		params.Set("cursor", *options.Cursor)
	}
	return params
}

func (c *Client) request(ctx context.Context, path string, params url.Values, result any) error {
	ctx, cancel := context.WithTimeout(ctx, c.timeout)
	defer cancel()
	endpoint := *c.baseURL
	endpoint.Path += path
	endpoint.RawQuery = params.Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint.String(), nil)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "proa-docs-go/0.1")
	response, err := c.http.Do(req)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	body, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes+1))
	if err != nil {
		return err
	}
	if len(body) > maxResponseBytes {
		return errors.New("Proa response exceeds 16 MiB")
	}
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		failure := &APIError{StatusCode: response.StatusCode}
		var problem Problem
		if json.Unmarshal(body, &problem) == nil {
			failure.Problem = &problem
		}
		seconds, parseErr := strconv.ParseInt(response.Header.Get("Retry-After"), 10, 64)
		if parseErr == nil && seconds >= 0 && seconds <= int64((1<<63-1)/time.Second) {
			delay := time.Duration(seconds) * time.Second
			failure.RetryAfter = &delay
		}
		return failure
	}
	if err := json.Unmarshal(body, result); err != nil {
		return fmt.Errorf("invalid Proa JSON response: %w", err)
	}
	return nil
}
