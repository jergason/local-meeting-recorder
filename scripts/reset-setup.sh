#!/bin/bash
# Reset the app to first-run state for testing

# macOS uses ~/Library/Application Support, Linux uses ~/.local/share
if [[ "$OSTYPE" == "darwin"* ]]; then
    CONFIG_DIR="$HOME/Library/Application Support/meeting-recorder"
else
    CONFIG_DIR="$HOME/.local/share/meeting-recorder"
fi

echo "Resetting Meeting Recorder setup..."

if [ -f "$CONFIG_DIR/config.json" ]; then
    rm "$CONFIG_DIR/config.json"
    echo "✓ Deleted config.json"
else
    echo "  config.json not found (already reset)"
fi

# Optionally clear models too
if [ "$1" = "--clear-models" ]; then
    if [ -d "$CONFIG_DIR/models" ]; then
        rm -rf "$CONFIG_DIR/models"
        echo "✓ Deleted models directory"
    else
        echo "  models directory not found"
    fi
fi

echo ""
echo "Done! Run 'pnpm tauri dev' to see the setup wizard again."
echo ""
echo "Tip: Use --clear-models to also delete downloaded models"
