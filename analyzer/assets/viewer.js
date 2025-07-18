import * as THREE from "https://esm.sh/three@0.160.0";
import { OrbitControls } from "https://esm.sh/three@0.160.0/examples/jsm/controls/OrbitControls.js";
import { OBJLoader } from "https://esm.sh/three@0.160.0/examples/jsm/loaders/OBJLoader.js";
import { GLTFLoader } from "https://esm.sh/three@0.160.0/examples/jsm/loaders/GLTFLoader.js";
import { DRACOLoader } from "https://esm.sh/three@0.160.0/examples/jsm/loaders/DRACOLoader.js";
import * as BufferGeometryUtils from 'https://esm.sh/three@0.160.0/examples/jsm/utils/BufferGeometryUtils.js';
import { MTLLoader } from "https://esm.sh/three@0.160.0/examples/jsm/loaders/MTLLoader.js";

const threeCDN = 'https://cdn.jsdelivr.net/npm/three@0.155.0/';
window.__THREE_CDN__ = threeCDN;

export function setupComparison(originalPath, resultPath, fileType = 'obj') {
  window.addEventListener('DOMContentLoaded', () => {
    const viewers = [
      { containerId: 'input_view', filePath: originalPath, mtlPath: './output.mtl', fileType: fileType },
      { containerId: 'output_view', filePath: resultPath, mtlPath: './output.mtl', fileType: fileType }
    ];

    viewers.forEach(viewer => {
      setupViewer(viewer.containerId, viewer.filePath, viewer.mtlPath, viewer.fileType);
    });

    const toggleNormals = document.getElementById('toggle_normals');
    const toggleClersSymbols = document.getElementById('toggle_clers_symbols');

    toggleNormals.addEventListener('change', () => {
      viewers.forEach(viewer => toggleViewerNormals(viewer.containerId, toggleNormals.checked));
    });

    toggleClersSymbols.addEventListener('change', () => {
      viewers.forEach(viewer => toggleViewerClersSymbols(viewer.containerId, toggleClersSymbols.checked));
    });
  });
}

function toggleViewerNormals(containerId, isVisible) {
  const container = document.getElementById(containerId);
  if (!container) return;

  // Toggle corner normal visualization (temporarily disabled)
  // if (container.cornerNormalMesh) {
  //   container.cornerNormalMesh.visible = isVisible;
  // }
}

function toggleViewerClersSymbols(containerId, isEnabled) {
  const container = document.getElementById(containerId);
  if (!container) return;

  const loadedMesh = container.loadedMesh;
  if (loadedMesh) {
    loadedMesh.traverse(child => {
      if (child.isMesh) {
        if (isEnabled) {
          // Reapply the original material from the MTL file
          if (child.originalMaterial) {
            child.material = child.originalMaterial;
            child.material.needsUpdate = true;
          }
        } else {
          // Store the original material and replace it with a default material
          if (!child.originalMaterial) {
            child.originalMaterial = child.material;
          }
          child.material = new THREE.MeshStandardMaterial({
            color: 0xaaaaaa, // Default gray color
            metalness: 0.5, // Add some metallic effect
            roughness: 0.8  // Add some roughness for better lighting
          });
          child.material.needsUpdate = true; // Ensure the material is updated
        }
      }
    });
  }
}

function setupViewer(containerId, filePath, mtlPath, fileType = 'obj') {
  const container = document.getElementById(containerId);
  if (!container) {
    console.error(`Container #${containerId} not found`);
    return;
  }

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0xf0f0f0); // Light gray background
  const aspect = container.clientWidth / container.clientHeight;
  const camera = new THREE.PerspectiveCamera(75, aspect, 0.1, 10000);
  
  // Initialize WebGL renderer with error handling
  const renderer = new THREE.WebGLRenderer({
    antialias: true,
    alpha: true
  });
  
  // Check for WebGL support
  if (!renderer.getContext()) {
    console.error('WebGL not supported');
    container.innerHTML = '<p>WebGL is not supported in your browser.</p>';
    return;
  }
  
  renderer.setSize(container.clientWidth, container.clientHeight);
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  container.appendChild(renderer.domElement);

  const controls = new OrbitControls(camera, renderer.domElement);

  // Add lights
  const ambientLight = new THREE.AmbientLight(0x404040, 1.5); // Soft ambient light
  scene.add(ambientLight);

  const directionalLight1 = new THREE.DirectionalLight(0xffffff, 1);
  directionalLight1.position.set(1, 1, 1).normalize();
  scene.add(directionalLight1);

  const directionalLight2 = new THREE.DirectionalLight(0xffffff, 0.5);
  directionalLight2.position.set(-1, -1, -1).normalize();
  scene.add(directionalLight2);

  container.loadedMesh = null; // Store the loaded mesh

  const slider = document.getElementById('face_slider');
  const sliderLabel = document.getElementById('face_slider_label');

  if (fileType === 'gltf') {
    // Load glTF/GLB files with Draco support
    const dracoLoader = new DRACOLoader();
    dracoLoader.setDecoderPath('https://www.gstatic.com/draco/versioned/decoders/1.5.7/');
    dracoLoader.preload();
    
    const gltfLoader = new GLTFLoader();
    gltfLoader.setDRACOLoader(dracoLoader);
    
    gltfLoader.load(filePath, gltf => {
      const object = gltf.scene;
      processLoadedObject(object, scene, camera, controls, container, slider, sliderLabel);
    }, progress => {
      // Progress callback
    }, error => {
      console.error('Error loading glTF:', error);
    });
  } else {
    // Load OBJ files with MTL materials
    const mtlLoader = new MTLLoader();
    mtlLoader.load(mtlPath, materials => {
      materials.preload();
      const objLoader = new OBJLoader();
      objLoader.setMaterials(materials);
      objLoader.load(filePath, object => {
        processLoadedObject(object, scene, camera, controls, container, slider, sliderLabel);
      }, undefined, error => {
        console.error('Error loading OBJ:', error);
      });
    }, undefined, error => {
      console.error('Error loading MTL:', error);
      // Fallback: load OBJ without materials
      const objLoader = new OBJLoader();
      objLoader.load(filePath, object => {
        processLoadedObject(object, scene, camera, controls, container, slider, sliderLabel);
      });
    });
  }

  slider.addEventListener('input', () => {
    const faceCount = parseInt(slider.value, 10);
    sliderLabel.textContent = `Faces: ${faceCount}`;
    updateMeshFaces(container.loadedMesh, faceCount);
  });

  function animate() {
    requestAnimationFrame(animate);
    controls.update();
    renderer.render(scene, camera);
  }
  animate();
}

// Create a mesh that visualizes normals at each corner of triangles with different colors
function createCornerNormalVisualization(mesh) {
  if (!mesh || !mesh.geometry) return null;

  const geometry = mesh.geometry;
  const positionAttribute = geometry.attributes.position;
  const normalAttribute = geometry.attributes.normal;
  
  if (!positionAttribute || !normalAttribute) return null;

  // Colors for the three corners of each triangle
  const cornerColors = [
    new THREE.Color(1, 0, 0), // Red for first corner
    new THREE.Color(0, 1, 0), // Green for second corner
    new THREE.Color(0, 0, 1)  // Blue for third corner
  ];

  // Calculate sphere radius based on mesh bounding box
  const boundingBox = new THREE.Box3().setFromBufferAttribute(positionAttribute);
  const meshSize = boundingBox.getSize(new THREE.Vector3()).length();
  let sphereRadius = meshSize * 0.01; // 1% of mesh size
  
  // Ensure sphere radius is reasonable (not too large or too small)
  sphereRadius = Math.max(0.1, Math.min(sphereRadius, 5));
  
  const sphereGeometry = new THREE.SphereGeometry(sphereRadius, 8, 8);
  const mergedGeometries = [];

  // Process each triangle
  const indexAttribute = geometry.index;
  const faceCount = indexAttribute ? indexAttribute.count / 3 : positionAttribute.count / 3;

  for (let faceIdx = 0; faceIdx < faceCount; faceIdx++) {
    for (let cornerIdx = 0; cornerIdx < 3; cornerIdx++) {
      let vertexIdx;
      
      if (indexAttribute) {
        vertexIdx = indexAttribute.getX(faceIdx * 3 + cornerIdx);
      } else {
        vertexIdx = faceIdx * 3 + cornerIdx;
      }

      // Get position directly from buffer attribute (already in object space)
      const position = new THREE.Vector3();
      position.fromBufferAttribute(positionAttribute, vertexIdx);
      
      // Create a small sphere at this corner
      const sphereClone = sphereGeometry.clone();
      sphereClone.translate(position.x, position.y, position.z);
      
      // Add color attribute
      const color = cornerColors[cornerIdx];
      const colors = new Float32Array(sphereClone.attributes.position.count * 3);
      for (let i = 0; i < sphereClone.attributes.position.count; i++) {
        colors[i * 3] = color.r;
        colors[i * 3 + 1] = color.g;
        colors[i * 3 + 2] = color.b;
      }
      sphereClone.setAttribute('color', new THREE.BufferAttribute(colors, 3));
      
      mergedGeometries.push(sphereClone);
    }
  }

  // Merge all spheres into one geometry for performance
  const mergedGeometry = BufferGeometryUtils.mergeGeometries(mergedGeometries);
  
  // Create material that uses vertex colors
  const material = new THREE.MeshBasicMaterial({
    vertexColors: true,
    transparent: true,
    opacity: 0.8
  });

  const cornerNormalMesh = new THREE.Mesh(mergedGeometry, material);
  cornerNormalMesh.visible = false; // Start hidden
  
  return cornerNormalMesh;
}

function processLoadedObject(object, scene, camera, controls, container, slider, sliderLabel) {
  object.traverse(child => {
    if (child.isMesh) {
      
      // Ensure the mesh has a material
      if (!child.material) {
        console.warn('Mesh has no material, creating default material');
        child.material = new THREE.MeshStandardMaterial({
          color: 0x808080,
          metalness: 0.5,
          roughness: 0.5
        });
      }
      
      
      // Store the original material right away
      if (!child.originalMaterial) {
        if (Array.isArray(child.material)) {
          child.originalMaterial = child.material.map(m => (m.clone ? m.clone() : m));
        } else if (child.material && child.material.clone) {
          child.originalMaterial = child.material.clone();
        } else {
          child.originalMaterial = child.material;
        }
      }

      // Ensure the geometry has proper attributes
      if (!child.geometry.attributes.position) {
        console.warn('Geometry missing position attribute');
        return;
      }

      // Ensure normals exist before creating normal helper
      if (!child.geometry.attributes.normal) {
        child.geometry.computeVertexNormals();
      }

      child.geometry.computeBoundingBox();
      container.loadedMesh = child; // Store the loaded mesh
      
      // Store container reference in the scene for updateMeshFaces
      scene.userData.container = container;

      // Ensure the geometry has an index
      if (!child.geometry.index) {
        child.geometry = BufferGeometryUtils.mergeVertices(child.geometry);
      }

      // Create corner normal visualization (temporarily disabled)
      // if (child.geometry.attributes.position && child.geometry.attributes.normal) {
      //   const cornerNormalMesh = createCornerNormalVisualization(child);
      //   if (cornerNormalMesh) {
      //     scene.add(cornerNormalMesh);
      //     container.cornerNormalMesh = cornerNormalMesh;
      //   }
      // }
    }
  });

  const box = new THREE.Box3().setFromObject(object);
  const size = box.getSize(new THREE.Vector3()).length();
  const center = box.getCenter(new THREE.Vector3());
  
  // Handle extreme sizes by scaling the object if needed
  if (size > 10000 || size < 0.01) {
    const targetSize = 100;
    const scaleFactor = targetSize / size;
    object.scale.multiplyScalar(scaleFactor);
    
    // Recalculate bounding box after scaling
    box.setFromObject(object);
    const newSize = box.getSize(new THREE.Vector3()).length();
    const newCenter = box.getCenter(new THREE.Vector3());
    
    object.position.sub(newCenter);
    camera.position.z = newSize * 1.5;
  } else {
    object.position.sub(center);
    camera.position.z = size * 1.5;
  }
  
  controls.update();
  scene.add(object);
  
  // Add axes helper to show coordinate system
  const axesHelper = new THREE.AxesHelper(50);
  scene.add(axesHelper);

  // Update slider max value based on the number of faces
  if (container.loadedMesh && container.loadedMesh.geometry.index) {
    const faceCount = container.loadedMesh.geometry.index.count / 3;
    slider.max = faceCount;
    slider.value = faceCount;
    sliderLabel.textContent = `Faces: ${faceCount}`;
  }
}

function updateMeshFaces(mesh, faceCount) {
  if (!mesh || !mesh.geometry || !mesh.geometry.index) return;

  let geometry = mesh.geometry;

  // Save the original index if it isn't already saved
  if (!mesh.userData.originalIndex) {
    mesh.userData.originalIndex = geometry.index.array.slice();
  }

  // Clamp faceCount to valid range
  const maxFaces = mesh.userData.originalIndex.length / 3;
  const actualFaceCount = Math.min(Math.max(0, faceCount), maxFaces);
  
  // Use the saved original index array
  const newIndex = mesh.userData.originalIndex.slice(0, actualFaceCount * 3);
  
  // Create new buffer attribute with proper size validation
  if (newIndex.length > 0) {
    geometry.setIndex(new THREE.BufferAttribute(newIndex, 1));
    geometry.computeVertexNormals();
    geometry.attributes.position.needsUpdate = true;
    geometry.attributes.normal.needsUpdate = true;
  }

  // Update corner normal visualization
  const container = mesh.parent?.userData?.container;
  if (container && container.cornerNormalMesh) {
    // Remove old visualization
    container.cornerNormalMesh.parent.remove(container.cornerNormalMesh);
    container.cornerNormalMesh.geometry.dispose();
    container.cornerNormalMesh.material.dispose();
    
    // Create new visualization with updated face count
    const newCornerNormalMesh = createCornerNormalVisualization(mesh);
    if (newCornerNormalMesh) {
      // Add to the same parent as the removed mesh
      container.cornerNormalMesh.parent.add(newCornerNormalMesh);
      container.cornerNormalMesh = newCornerNormalMesh;
      
      // Maintain visibility state
      const toggleNormals = document.getElementById('toggle_normals');
      if (toggleNormals) {
        newCornerNormalMesh.visible = toggleNormals.checked;
      }
    }
  }
}
