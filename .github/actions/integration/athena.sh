#!/bin/bash
set -eo pipefail

# Debug log for test containers
export DEBUG=testcontainers

echo "::group::Athena [cloud]"
export CUBEJS_AWS_KEY=$DRIVERS_TESTS_SNOWFLAKE_CUBEJS_AWS_KEY
export CUBEJS_AWS_SECRET=$DRIVERS_TESTS_SNOWFLAKE_CUBEJS_AWS_SECRET

export CUBEJS_AWS_REGION=us-east-1
export CUBEJS_AWS_S3_OUTPUT_LOCATION=s3://cubejs-opensource/testing/output
export CUBEJS_DB_EXPORT_BUCKET=s3://cubejs-opensource/testing/export/
yarn lerna run --concurrency 1 --stream --no-prefix integration:athena
export CUBEJS_DB_EXPORT_BUCKET=cubejs-opensource
yarn lerna run --concurrency 1 --stream --no-prefix integration:athena
# yarn lerna run --concurrency 1 --stream --no-prefix smoke:athena
echo "::endgroup::"
