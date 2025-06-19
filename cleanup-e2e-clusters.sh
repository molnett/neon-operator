#!/bin/bash

# Cleanup script for e2e test clusters

echo "🧹 Cleaning up e2e test clusters..."

# Find all Kind clusters that start with neon-e2e-
clusters=$(kind get clusters 2>/dev/null | grep "^neon-e2e-" || true)

if [ -z "$clusters" ]; then
    echo "✅ No e2e test clusters found to cleanup"
    exit 0
fi

echo "Found the following e2e test clusters:"
echo "$clusters"
echo ""

for cluster in $clusters; do
    echo "🗑️  Deleting cluster: $cluster"
    if kind delete cluster --name "$cluster"; then
        echo "✅ Successfully deleted $cluster"
    else
        echo "❌ Failed to delete $cluster"
    fi
    echo ""
done

echo "🧹 Cleanup completed!"