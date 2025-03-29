use crate::shared::attribute::Portable;
use crate::core::shared::Vector;

pub(crate) trait PredictionTransform {
	const ID: usize = 0;
	/// The data type that needs to store.
	type Data: Vector;
	type Correction: Portable;
	type Metadata: Portable;
	
	/// transforms the data (the correction value) with the given metadata.
	fn map(orig: Self::Data, pred: Self::Data, metadata: Self::Metadata);

	/// transforms the data (the correction value) with the tentative metadata value.
	/// The tentative metadata can be determined by the function without any restriction,
	/// but it needs to be returned. The output of the transform might get later on 
	/// fixed by the metadata universal to the attribute after all the transforms are
	/// done once for each attribute value.
	fn map_with_tentative_metadata(orig: Self::Data, pred: Self::Data);

	/// The inverse transform revertes 'map()'.
	fn inverse(pred: Self::Data, crr: Self::Correction, metadata: Self::Metadata);
	
	/// squeezes the transform results having computed the entire attribute and
	/// gives up the final data.
	/// This includes cutting off the unnecessary data from both tentative metadata
	/// and the transformed data, or doing some trade-off's between the tentative
	/// metadata and the transformed data to decide the global metadata that will 
	/// be encoded to buffer.
	fn squeeze(self) -> impl Iterator<Item = Self::Correction>;
}

/// Trait limiting the selections of the encoding methods for vertex coordinates.
trait TransformForVertexCoords: PredictionTransform {}

/// Trait limiting the selections of the encoding methods for texture coordinates.
trait TransformForTexCoords: PredictionTransform {}

/// Trait limiting the selections of the encoding methods for normals.
trait TansformForNormals: PredictionTransform {}