#!/usr/bin/env bash
# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

# One-time setup, run by a project administrator. No credentials are written.
set -euo pipefail
retry() {
  local delay
  for delay in 0 2 4 8 16 30; do
    sleep "$delay"
    if "$@"; then return 0; fi
  done
  return 1
}
project=airy-cycle-504821-k9
builder="proa-marketing-builder@$project.iam.gserviceaccount.com"
runner="proa-ci-sccache@$project.iam.gserviceaccount.com"
puller="proa-railway-pull@$project.iam.gserviceaccount.com"
bucket="gs://$project-marketing-cache"
gcloud iam service-accounts describe "$builder" --project "$project" >/dev/null 2>&1 || \
  gcloud iam service-accounts create proa-marketing-builder --project "$project" --display-name='Marketing release image builder'
gcloud iam service-accounts describe "$puller" --project "$project" >/dev/null 2>&1 || \
  gcloud iam service-accounts create proa-railway-pull --project "$project" --display-name='Railway release image puller'
gcloud storage buckets describe "$bucket" >/dev/null 2>&1 || \
  gcloud storage buckets create "$bucket" --location=us-west1 --uniform-bucket-level-access
gcloud artifacts repositories describe releases --project "$project" --location us-west1 >/dev/null 2>&1 || \
  gcloud artifacts repositories create releases --project "$project" --location us-west1 --repository-format=docker
retry gcloud artifacts repositories add-iam-policy-binding releases --project "$project" --location us-west1 \
  --member="serviceAccount:$builder" --role=roles/artifactregistry.writer >/dev/null
# The organization disallows public registry access. Railway's dedicated
# identity can only read this repository; it cannot write images or caches.
retry gcloud artifacts repositories add-iam-policy-binding releases --project "$project" --location us-west1 \
  --member="serviceAccount:$puller" --role=roles/artifactregistry.reader >/dev/null
for account in "$builder" "$runner"; do
  gcloud storage buckets add-iam-policy-binding "$bucket" \
    --member="serviceAccount:$account" --role=roles/storage.objectAdmin >/dev/null
done
gcloud projects add-iam-policy-binding "$project" \
  --member="serviceAccount:$builder" --role=roles/logging.logWriter --condition=None >/dev/null
gcloud projects add-iam-policy-binding "$project" \
  --member="serviceAccount:$runner" --role=roles/cloudbuild.builds.editor --condition=None >/dev/null
gcloud iam service-accounts add-iam-policy-binding "$builder" --project "$project" \
  --member="serviceAccount:$runner" --role=roles/iam.serviceAccountUser >/dev/null
gcloud iam service-accounts add-iam-policy-binding "$puller" --project "$project" \
  --member="serviceAccount:$runner" --role=roles/iam.serviceAccountTokenCreator >/dev/null
