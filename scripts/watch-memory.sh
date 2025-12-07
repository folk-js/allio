#!/bin/bash
# Watch memory usage of the axio app while it's running
# Usage: ./scripts/watch-memory.sh
#
# In another terminal, run: npm run dev

INTERVAL=2

# Find the debug build process (not the bundled release app)
find_axio_pid() {
    # Look for the debug binary specifically
    ps aux | grep "[t]arget/debug/axio" | awk '{print $2}' | head -1
}

PID=$(find_axio_pid)

if [ -z "$PID" ]; then
    echo "Waiting for axio to start..."
    echo "(Run 'npm run dev' in another terminal)"
    echo ""
    while true; do
        PID=$(find_axio_pid)
        if [ -n "$PID" ]; then
            break
        fi
        sleep 0.5
    done
fi

echo "Found axio (PID: $PID)"
echo ""
printf "%-10s %-12s %-12s %-10s\n" "Time" "RSS (MB)" "Threads" "Δ RSS"
echo "----------------------------------------"

LAST_RSS=0
START_TIME=$(date +%s)

while true; do
    PID=$(find_axio_pid)
    if [ -z "$PID" ]; then
        echo ""
        echo "⚠️  Process ended"
        break
    fi
    
    # Get RSS in KB
    RSS_KB=$(ps -o rss= -p $PID 2>/dev/null | tr -d ' ')
    if [ -z "$RSS_KB" ] || [ "$RSS_KB" = "0" ]; then
        echo "⚠️  Process ended"
        break
    fi
    
    # Get thread count
    THREADS=$(ps -M $PID 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
    
    # Calculate values
    RSS_MB=$(awk "BEGIN {printf \"%.1f\", $RSS_KB / 1024}")
    ELAPSED=$(($(date +%s) - START_TIME))
    
    # Calculate delta
    if [ "$LAST_RSS" -ne 0 ]; then
        DELTA_KB=$((RSS_KB - LAST_RSS))
        DELTA_MB=$(awk "BEGIN {printf \"%.2f\", $DELTA_KB / 1024}")
        if [ "$DELTA_KB" -gt 0 ]; then
            DELTA="+${DELTA_MB}"
        elif [ "$DELTA_KB" -lt 0 ]; then
            DELTA="$DELTA_MB"
        else
            DELTA="0"
        fi
    else
        DELTA="-"
    fi
    LAST_RSS=$RSS_KB
    
    printf "%-10s %-12s %-12s %-10s\n" "${ELAPSED}s" "$RSS_MB" "$THREADS" "$DELTA"
    
    sleep $INTERVAL
done

