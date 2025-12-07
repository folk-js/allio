#!/bin/bash
# Check for memory leaks in the running axio app
# Usage: ./scripts/check-leaks.sh
#
# In another terminal, run: npm run dev

echo "Leak Checker for axio"
echo "=============================="

# Find the debug build process
PID=$(ps aux | grep "[t]arget/debug/axio" | awk '{print $2}' | head -1)

if [ -z "$PID" ]; then
    echo "⚠️  axio is not running"
    echo "   Start it with: npm run dev"
    exit 1
fi

echo "Checking PID: $PID"
echo ""

# Run leaks command
leaks $PID 2>&1 | head -50

echo ""
echo "---"
echo "For full output, run: leaks $PID"

