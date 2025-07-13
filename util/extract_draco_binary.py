#!/usr/bin/env python3
"""
Extract Draco binary data from a GLB file containing Draco-compressed meshes.

This script parses a GLB file and extracts the Draco-compressed binary data
from the buffers, which can then be used for analysis or decompression.
"""

import struct
import json
import sys
import argparse
from pathlib import Path


class GLBParser:
    def __init__(self, glb_path):
        self.glb_path = Path(glb_path)
        self.header = None
        self.json_chunk = None
        self.binary_chunk = None
        
    def parse(self):
        """Parse the GLB file and extract JSON and binary chunks."""
        with open(self.glb_path, 'rb') as f:
            # Read GLB header (12 bytes)
            magic = f.read(4)
            if magic != b'glTF':
                raise ValueError("Not a valid GLB file - missing glTF magic")
            
            version = struct.unpack('<I', f.read(4))[0]
            if version != 2:
                raise ValueError(f"Unsupported GLB version: {version}")
            
            total_length = struct.unpack('<I', f.read(4))[0]
            
            # Read JSON chunk
            json_chunk_length = struct.unpack('<I', f.read(4))[0]
            json_chunk_type = f.read(4)
            if json_chunk_type != b'JSON':
                raise ValueError("Expected JSON chunk")
            
            json_data = f.read(json_chunk_length).decode('utf-8')
            self.json_chunk = json.loads(json_data)
            
            # Read binary chunk if present
            if f.tell() < total_length:
                binary_chunk_length = struct.unpack('<I', f.read(4))[0]
                binary_chunk_type = f.read(4)
                if binary_chunk_type != b'BIN\x00':
                    raise ValueError("Expected BIN chunk")
                
                self.binary_chunk = f.read(binary_chunk_length)
    
    def find_draco_buffers(self):
        """Find buffers that contain Draco-compressed data."""
        draco_buffers = []
        
        # Look for meshes with Draco compression
        if 'meshes' in self.json_chunk:
            for mesh_idx, mesh in enumerate(self.json_chunk['meshes']):
                for prim_idx, primitive in enumerate(mesh.get('primitives', [])):
                    # Check for KHR_draco_mesh_compression extension
                    extensions = primitive.get('extensions', {})
                    draco_ext = extensions.get('KHR_draco_mesh_compression')
                    
                    if draco_ext:
                        buffer_view_idx = draco_ext.get('bufferView')
                        if buffer_view_idx is not None:
                            draco_buffers.append({
                                'mesh_index': mesh_idx,
                                'primitive_index': prim_idx,
                                'buffer_view_index': buffer_view_idx,
                                'attributes': draco_ext.get('attributes', {}),
                                'indices': draco_ext.get('indices')
                            })
        
        return draco_buffers
    
    def extract_draco_binary(self, buffer_view_idx):
        """Extract Draco binary data from the specified buffer view."""
        if not self.binary_chunk:
            raise ValueError("No binary chunk found in GLB file")
        
        buffer_views = self.json_chunk.get('bufferViews', [])
        if buffer_view_idx >= len(buffer_views):
            raise ValueError(f"Buffer view index {buffer_view_idx} out of range")
        
        buffer_view = buffer_views[buffer_view_idx]
        buffer_idx = buffer_view.get('buffer', 0)
        byte_offset = buffer_view.get('byteOffset', 0)
        byte_length = buffer_view.get('byteLength')
        
        if buffer_idx != 0:
            raise ValueError(f"External buffers not supported (buffer index: {buffer_idx})")
        
        if byte_length is None:
            raise ValueError("Buffer view missing byteLength")
        
        end_offset = byte_offset + byte_length
        if end_offset > len(self.binary_chunk):
            raise ValueError("Buffer view extends beyond binary chunk")
        
        return self.binary_chunk[byte_offset:end_offset]


def main():
    parser = argparse.ArgumentParser(
        description="Extract Draco binary data from GLB files"
    )
    parser.add_argument("input", help="Input GLB file path")
    parser.add_argument("-o", "--output", help="Output directory for Draco binaries")
    parser.add_argument("-i", "--info", action="store_true", 
                       help="Show information about Draco buffers without extracting")
    
    args = parser.parse_args()
    
    try:
        glb_parser = GLBParser(args.input)
        glb_parser.parse()
        
        draco_buffers = glb_parser.find_draco_buffers()
        
        if not draco_buffers:
            print("No Draco-compressed meshes found in GLB file")
            return 1
        
        print(f"Found {len(draco_buffers)} Draco-compressed mesh(es):")
        
        for i, draco_info in enumerate(draco_buffers):
            print(f"\nMesh {draco_info['mesh_index']}, Primitive {draco_info['primitive_index']}:")
            print(f"  Buffer view index: {draco_info['buffer_view_index']}")
            print(f"  Attributes: {draco_info['attributes']}")
            if draco_info['indices'] is not None:
                print(f"  Indices: {draco_info['indices']}")
            
            if not args.info:
                # Extract the binary data
                draco_binary = glb_parser.extract_draco_binary(draco_info['buffer_view_index'])
                
                # Determine output filename
                if args.output:
                    output_dir = Path(args.output)
                    output_dir.mkdir(exist_ok=True)
                else:
                    output_dir = Path.cwd()
                
                output_filename = f"mesh_{draco_info['mesh_index']}_prim_{draco_info['primitive_index']}.drc"
                output_path = output_dir / output_filename
                
                # Write the Draco binary
                with open(output_path, 'wb') as f:
                    f.write(draco_binary)
                
                print(f"  Extracted to: {output_path}")
                print(f"  Size: {len(draco_binary)} bytes")
        
        return 0
        
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())