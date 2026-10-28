# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Read-only, credential-free client for the Proa documentation API."""

from .client import Document, DocumentList, DocumentSummary, ProaApiError, ProaDocs

__all__ = ["Document", "DocumentList", "DocumentSummary", "ProaApiError", "ProaDocs"]
__version__ = "0.1.1"
