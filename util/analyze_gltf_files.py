#!/usr/bin/env python3

import os
import sys
import subprocess
import argparse
from pathlib import Path

def find_gltf_files(directory, recursive=False):
    """Find all .gltf and .glb files in the given directory."""
    extensions = ['.gltf', '.glb']
    files = []
    
    if recursive:
        for ext in extensions:
            files.extend(Path(directory).rglob(f'*{ext}'))
    else:
        for ext in extensions:
            files.extend(Path(directory).glob(f'*{ext}'))
    
    return sorted(files)

def run_analyzer(file_path):
    """Run the analyzer on a single file."""
    cmd = ['cargo', 'run', '--release', '--bin', 'analyzer', '--', '--original', str(file_path)]
    
    print(f"\nAnalyzing: {file_path}")
    print(f"Command: {' '.join(cmd)}")
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True)
        
        if result.returncode == 0:
            print(f"✓ Successfully analyzed {file_path}")
            if result.stdout:
                print("Output:", result.stdout[:200] + "..." if len(result.stdout) > 200 else result.stdout)
        else:
            print(f"✗ Failed to analyze {file_path}")
            if result.stderr:
                print("Error:", result.stderr)
    except Exception as e:
        print(f"✗ Exception while analyzing {file_path}: {e}")

def main():
    parser = argparse.ArgumentParser(description='Analyze all glTF/GLB files in a directory using the draco-rs analyzer')
    parser.add_argument('directory', help='Directory containing glTF/GLB files')
    parser.add_argument('-r', '--recursive', action='store_true', help='Search for files recursively')
    
    args = parser.parse_args()
    
    if not os.path.isdir(args.directory):
        print(f"Error: '{args.directory}' is not a valid directory")
        sys.exit(1)
    
    files = find_gltf_files(args.directory, args.recursive)
    
    if not files:
        print(f"No glTF/GLB files found in '{args.directory}'")
        if not args.recursive:
            print("Tip: Use -r flag to search recursively")
        sys.exit(0)
    
    print(f"Found {len(files)} glTF/GLB file(s)")
    
    for file in files:
        run_analyzer(file)
    
    print(f"\nCompleted analyzing {len(files)} file(s)")

if __name__ == '__main__':
    main()