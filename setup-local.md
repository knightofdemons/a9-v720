# A9 V720 Project Setup

## Project Overview
This is a local copy of the A9 V720 (naxclow) camera tool project. The original repository is available at: https://github.com/intx82/a9-v720

## Project Structure
- `src/` - Main Python source code
- `docs/` - Documentation and data sheets
- `img/` - Camera hardware images
- `orig-app/` - Original Android app files
- `static/` - Web interface files
- `a9_old.py` - Early version of the script
- `fake_server.md` - Fake server documentation
- `readme.md` - Main project documentation

## Key Features
- **AP Mode**: Connect directly to camera's WiFi hotspot
- **STA Mode**: Connect camera to existing WiFi network
- **Live Streaming**: Real-time video streaming
- **Fake Server**: Local server to intercept camera communications
- **Snapshot Capture**: Take photos from the camera

## Usage Modes

### AP Mode (Direct Connection)
```bash
python3 src/a9_naxclow.py -l -o live.avi -r -i
```

### STA Mode (Network Connection)
```bash
# Set WiFi credentials
python3 src/a9_naxclow.py --set-wifi [SSID] [PWD]

# Start fake server
python3 src/a9_naxclow.py -s
```

## Dependencies
Install required packages:
```bash
pip install -r requirements.txt
```

## Agent Reload Instructions
When reloading the agent, this file provides context about:
1. Project purpose and structure
2. Available functionality
3. Usage patterns
4. Key files and their purposes

## Recent Changes
- Repository cloned fresh to `../a9-v720-fresh/` for latest version
- Current workspace contains project files
- Setup-local.md created for agent context

## Notes
- Tested with camera version `202212011602`
- Uses Chinese V720 app in AP mode
- Requires DNS redirection for fake server functionality
- Camera IP range: 192.168.169.x (AP mode) 