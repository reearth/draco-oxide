#!/usr/bin/env python3
"""
Extract JSON part from GLB files and save as separate JSON files.

GLB (GL Transmission Format Binary) files contain:
- 12-byte header
- JSON chunk (with chunk header)
- Binary chunk (optional, with chunk header)

This script extracts the JSON chunk and saves it with the same filename but .json extension.
"""

import struct
import json
import sys
import os
from pathlib import Path


def extract_json_from_glb(glb_path):
    """Extract JSON chunk from GLB file and return as dictionary."""
    with open(glb_path, 'rb') as f:
        # Read GLB header (12 bytes)
        magic = f.read(4)
        if magic != b'glTF':
            raise ValueError(f"Not a valid GLB file: {glb_path}")
        
        version = struct.unpack('<I', f.read(4))[0]
        if version != 2:
            raise ValueError(f"Unsupported GLB version: {version}")
        
        total_length = struct.unpack('<I', f.read(4))[0]
        
        # Read first chunk header (8 bytes)
        chunk_length = struct.unpack('<I', f.read(4))[0]
        chunk_type = f.read(4)
        
        if chunk_type != b'JSON':
            raise ValueError("First chunk is not JSON")
        
        # Read JSON data
        json_data = f.read(chunk_length)
        
        # Remove null padding if present
        json_data = json_data.rstrip(b'\x00')
        
        # Parse JSON
        return json.loads(json_data.decode('utf-8'))


def save_json_to_file(json_data, output_path):
    """Save JSON data to file with pretty formatting."""
    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump(json_data, f, indent=2, ensure_ascii=False)


def process_glb_file(glb_path):
    """Process a single GLB file and extract its JSON."""
    glb_path = Path(glb_path)
    
    if not glb_path.exists():
        print(f"Error: File not found: {glb_path}")
        return False
    
    if not glb_path.suffix.lower() == '.glb':
        print(f"Error: Not a GLB file: {glb_path}")
        return False
    
    # Generate output path (same directory, same name, .json extension)
    output_path = glb_path.with_suffix('.json')
    
    try:
        print(f"Processing: {glb_path}")
        json_data = extract_json_from_glb(glb_path)
        save_json_to_file(json_data, output_path)
        print(f"JSON extracted to: {output_path}")
        return True
    
    except Exception as e:
        print(f"Error processing {glb_path}: {e}")
        return False


def main():
    """Main function to handle command line arguments."""
    if len(sys.argv) < 2:
        print("Usage: python extract_glb_json.py <glb_file1> [glb_file2] ...")
        print("   or: python extract_glb_json.py *.glb")
        sys.exit(1)
    
    success_count = 0
    total_count = len(sys.argv) - 1
    
    for glb_file in sys.argv[1:]:
        if process_glb_file(glb_file):
            success_count += 1
    
    print(f"\nProcessed {success_count}/{total_count} files successfully.")


if __name__ == "__main__":
    main()