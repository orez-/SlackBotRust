#!/bin/bash
set -euxo pipefail

# The name of the stack
STACK_NAME="$1"
ARTIFACT_BUCKET="$2"
if [ "$#" -gt 2 ]; then
    SLACK_TOKEN="$3"
fi

# Ensure we have target platform
# XXX: OSX might have to install the linker as well?? Good luck, sorry.
rustup target add x86_64-unknown-linux-musl

# Build executable
cargo build --release --target x86_64-unknown-linux-musl

# Package executable
cp ./target/x86_64-unknown-linux-musl/release/slack_bot_rust ./target/bootstrap

aws cloudformation package \
    --template-file cloudformation.yml \
    --output-template-file cloudformation.out.yml \
    --s3-bucket "${ARTIFACT_BUCKET}"
if [ "$#" -gt 2 ]; then
    aws cloudformation deploy \
        --template-file cloudformation.out.yml \
        --stack-name "${STACK_NAME}" \
        --capabilities CAPABILITY_IAM \
        --parameter-overrides \
        "SlackToken=${SLACK_TOKEN}"
else
    aws cloudformation deploy \
        --template-file cloudformation.out.yml \
        --stack-name "${STACK_NAME}" \
        --capabilities CAPABILITY_IAM
fi

aws cloudformation describe-stacks --stack-name "${STACK_NAME}" \
    --query "Stacks[0].Outputs[?OutputKey=='WebhookUrl'].OutputValue" \
    --output text
