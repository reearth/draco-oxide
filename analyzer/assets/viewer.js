import * as THREE from "https://esm.sh/three@0.160.0";
import { OrbitControls } from "https://esm.sh/three@0.160.0/examples/jsm/controls/OrbitControls.js";
import { OBJLoader } from "https://esm.sh/three@0.160.0/examples/jsm/loaders/OBJLoader.js";
import * as BufferGeometryUtils from 'https://esm.sh/three@0.160.0/examples/jsm/utils/BufferGeometryUtils.js';
import { VertexNormalsHelper } from "https://esm.sh/three@0.160.0/examples/jsm/helpers/VertexNormalsHelper.js";
import { MTLLoader } from "https://esm.sh/three@0.160.0/examples/jsm/loaders/MTLLoader.js";

const threeCDN = 'https://cdn.jsdelivr.net/npm/three@0.155.0/';
window.__THREE_CDN__ = threeCDN;

export function setupComparison(originalPath, resultPath) {
  window.addEventListener('DOMContentLoaded', () => {
    const viewers = [
      { containerId: 'original_view', objPath: originalPath, mtlPath: './result.mtl' },
      { containerId: 'result_view', objPath: resultPath, mtlPath: './result.mtl' }
    ];

    viewers.forEach(viewer => {
      setupViewer(viewer.containerId, viewer.objPath, viewer.mtlPath);
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

  const normalHelpers = container.normalHelpers || [];
  normalHelpers.forEach(({ helper }) => {
    helper.visible = isVisible;
  });
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

function setupViewer(containerId, objPath, mtlPath) {
  const container = document.getElementById(containerId);
  if (!container) {
    console.error(`Container #${containerId} not found`);
    return;
  }

  const scene = new THREE.Scene();
  const aspect = container.clientWidth / container.clientHeight;
  const camera = new THREE.PerspectiveCamera(75, aspect, 0.1, 1000);
  const renderer = new THREE.WebGLRenderer();
  renderer.setSize(container.clientWidth, container.clientHeight);
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

  const mtlLoader = new MTLLoader();
  mtlLoader.load(mtlPath, materials => {
    materials.preload();
    const objLoader = new OBJLoader();
    objLoader.setMaterials(materials);
    objLoader.load(objPath, object => {
      object.traverse(child => {
        if (child.isMesh) {
          // Store the MTL-based material right away
          if (!child.originalMaterial) {
            if (Array.isArray(child.material)) {
              child.originalMaterial = child.material.map(m => (m.clone ? m.clone() : m));
            } else if (child.material && child.material.clone) {
              child.originalMaterial = child.material.clone();
            }
          }

          child.geometry.computeBoundingBox();
          container.loadedMesh = child; // Store the loaded mesh

          // Ensure the geometry has an index
          if (!child.geometry.index) {
            child.geometry = BufferGeometryUtils.mergeVertices(child.geometry); // Use BufferGeometryUtils to merge vertices
          }

          // Create a normal helper and store it in the container
          const normalHelper = new VertexNormalsHelper(child, 2, 0xff0000);
          scene.add(normalHelper);
          container.normalHelpers = container.normalHelpers || [];
          container.normalHelpers.push({ helper: normalHelper });
        }
      });

      const box = new THREE.Box3().setFromObject(object);
      const size = box.getSize(new THREE.Vector3()).length();
      const center = box.getCenter(new THREE.Vector3());
      object.position.sub(center);
      camera.position.z = size * 1.5;
      controls.update();
      scene.add(object);

      // Update slider max value based on the number of faces
      if (container.loadedMesh) {
        const faceCount = container.loadedMesh.geometry.index.count / 3;
        slider.max = faceCount;
        slider.value = faceCount;
        sliderLabel.textContent = `Faces: ${faceCount}`;
      }
    });
  });

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

function updateMeshFaces(mesh, faceCount) {
  if (!mesh) return;

  let geometry = mesh.geometry;

  // Save the original index if it isn't already saved
  if (!mesh.userData.originalIndex) {
    mesh.userData.originalIndex = geometry.index.array.slice();
  }

  // Use the saved original index array
  const newIndex = mesh.userData.originalIndex.slice(0, faceCount * 3);
  geometry.setIndex(new THREE.BufferAttribute(newIndex, 1));
  geometry.computeVertexNormals();
  geometry.needsUpdate = true;
}
