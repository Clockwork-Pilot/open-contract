#!/bin/sh

set -u

script_dir=$(CDPATH= cd "$(dirname "$0")" && pwd) || exit 2
open_contract_root=$(CDPATH= cd "$script_dir/../.." && pwd) || exit 2
cd "$open_contract_root" || exit 2

absolute_path() {
    case "$1" in
        /*) printf '%s\n' "$1" ;;
        *) printf '%s/%s\n' "$open_contract_root" "$1" ;;
    esac
}

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <bootstrap-contract> [<bootstrap-contract> ...]" >&2
    exit 2
fi

overall_status=0

for contract in "$@"; do
    case "$contract" in
        batch-execution|cli|dataset-generation|error-handling|isolation|manifest|verification)
            project=$contract
            check_goal="check-$contract-contract"
            profile=".coverage/bootstrap-contracts/$contract-dataset.profdata"
            ignore_regex='^(/|tests/)'
            if [ "$contract" = verification ]; then
                ignore_regex='^(/|tests/|bin/)'
            fi
            ;;
        dataset-harness)
            project=$contract
            check_goal=check-bootstrap-dataset-harness
            profile=".coverage/bootstrap-contracts/$contract-dataset.profdata"
            ignore_regex='^(/|tests/)'
            ;;
        predicate)
            project=predicates
            check_goal=check-predicate-contract
            profile="$HOME/.cache/open-contract-target/predicates/llvm-cov-target/predicates.profdata"
            ignore_regex='^(/|tests/)'
            ;;
        *)
            echo "unknown bootstrap contract: $contract" >&2
            overall_status=1
            continue
            ;;
    esac

    echo "Checking $contract contract and generating its HTML coverage report"
    make "$check_goal"
    check_status=$?

    if [ ! -f "$profile" ]; then
        echo "coverage report unavailable for $contract: profile was not produced at $(absolute_path "$profile")" >&2
        overall_status=1
        continue
    fi

    make bootstrap-contract-coverage-html \
        "COVERAGE_PROJECT=runner/bootstrap-contracts/$project" \
        "COVERAGE_PROFILE=$(absolute_path "$profile")" \
        "COVERAGE_BINARY_PATH=$(absolute_path ".coverage/bootstrap-contracts/$project-dataset-binary.path")" \
        "COVERAGE_IGNORE_REGEX=$ignore_regex" \
        "COVERAGE_HTML_DIR=$(absolute_path ".coverage/bootstrap-contracts/$project/html")" \
        "COVERAGE_SUMMARY=$(absolute_path ".coverage/bootstrap-contracts/$project/summary.txt")"
    report_status=$?

    if [ "$check_status" -ne 0 ] || [ "$report_status" -ne 0 ]; then
        overall_status=1
    fi
done

exit "$overall_status"
