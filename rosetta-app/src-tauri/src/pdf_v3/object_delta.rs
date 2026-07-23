use std::{collections::BTreeMap, fmt};

use lopdf::{Document, Object, ObjectId};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PdfObjectDelta {
    objects: BTreeMap<ObjectId, Object>,
    maximum_object_number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PdfObjectDeltaError {
    InvalidObjectId(ObjectId),
    DuplicateObjectNumber(u32),
    MaximumObjectNumberTooSmall { maximum: u32, object_number: u32 },
    ConflictingObject(ObjectId),
}

impl fmt::Display for PdfObjectDeltaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidObjectId((number, generation)) => {
                write!(
                    formatter,
                    "PDF delta object ID {number} {generation} is invalid"
                )
            }
            Self::DuplicateObjectNumber(number) => {
                write!(
                    formatter,
                    "PDF delta contains multiple generations for object {number}"
                )
            }
            Self::MaximumObjectNumberTooSmall {
                maximum,
                object_number,
            } => write!(
                formatter,
                "PDF delta maximum object number {maximum} is below object {object_number}"
            ),
            Self::ConflictingObject((number, generation)) => write!(
                formatter,
                "PDF delta object {number} {generation} has conflicting staged values"
            ),
        }
    }
}

impl std::error::Error for PdfObjectDeltaError {}

impl PdfObjectDelta {
    pub(crate) fn empty(maximum_object_number: u32) -> Self {
        Self {
            objects: BTreeMap::new(),
            maximum_object_number,
        }
    }

    pub(crate) fn try_from_objects(
        objects: BTreeMap<ObjectId, Object>,
        maximum_object_number: u32,
    ) -> Result<Self, PdfObjectDeltaError> {
        let mut previous_number = None;
        for &(number, generation) in objects.keys() {
            if number == 0 || generation == u16::MAX {
                return Err(PdfObjectDeltaError::InvalidObjectId((number, generation)));
            }
            if previous_number == Some(number) {
                return Err(PdfObjectDeltaError::DuplicateObjectNumber(number));
            }
            if number > maximum_object_number {
                return Err(PdfObjectDeltaError::MaximumObjectNumberTooSmall {
                    maximum: maximum_object_number,
                    object_number: number,
                });
            }
            previous_number = Some(number);
        }
        Ok(Self {
            objects,
            maximum_object_number,
        })
    }

    pub(crate) fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub(crate) fn maximum_object_number(&self) -> u32 {
        self.maximum_object_number
    }

    pub(crate) fn objects(&self) -> &BTreeMap<ObjectId, Object> {
        &self.objects
    }

    pub(crate) fn merge(&mut self, other: Self) -> Result<(), PdfObjectDeltaError> {
        for (&object_id, object) in &other.objects {
            match self.objects.get(&object_id) {
                Some(existing) if existing != object => {
                    return Err(PdfObjectDeltaError::ConflictingObject(object_id));
                }
                Some(_) => {}
                None if self
                    .objects
                    .keys()
                    .any(|candidate| candidate.0 == object_id.0) =>
                {
                    return Err(PdfObjectDeltaError::DuplicateObjectNumber(object_id.0));
                }
                None => {}
            }
        }
        for (object_id, object) in other.objects {
            if !self.objects.contains_key(&object_id) {
                self.objects.insert(object_id, object);
            }
        }
        self.maximum_object_number = self.maximum_object_number.max(other.maximum_object_number);
        Ok(())
    }

    pub(crate) fn apply_to(&self, document: &mut Document) {
        for (&object_id, object) in &self.objects {
            document.objects.insert(object_id, object.clone());
        }
        document.max_id = document.max_id.max(self.maximum_object_number);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use lopdf::{Document, Object};

    use super::{PdfObjectDelta, PdfObjectDeltaError};

    #[test]
    fn merges_idempotent_objects_and_rejects_conflicting_values() {
        let object = Object::string_literal("stable");
        let mut delta =
            PdfObjectDelta::try_from_objects(BTreeMap::from([((5, 0), object.clone())]), 5)
                .expect("first delta");
        delta
            .merge(
                PdfObjectDelta::try_from_objects(
                    BTreeMap::from([((5, 0), object), ((7, 0), Object::Integer(7))]),
                    7,
                )
                .expect("second delta"),
            )
            .expect("idempotent merge");
        assert_eq!(delta.object_count(), 2);
        assert_eq!(delta.maximum_object_number(), 7);

        let error = delta
            .merge(
                PdfObjectDelta::try_from_objects(
                    BTreeMap::from([((5, 0), Object::string_literal("changed"))]),
                    7,
                )
                .expect("conflicting delta"),
            )
            .expect_err("conflict");
        assert_eq!(error, PdfObjectDeltaError::ConflictingObject((5, 0)));
    }

    #[test]
    fn applies_only_staged_objects_and_advances_document_maximum() {
        let mut document = Document::new();
        document.max_id = 3;
        document.objects.insert((2, 0), Object::Integer(2));
        let delta = PdfObjectDelta::try_from_objects(
            BTreeMap::from([((2, 0), Object::Integer(20)), ((6, 0), Object::Integer(60))]),
            6,
        )
        .expect("delta");

        delta.apply_to(&mut document);

        assert_eq!(document.max_id, 6);
        assert_eq!(document.objects.len(), 2);
        assert_eq!(
            document.get_object((2, 0)).expect("updated object"),
            &Object::Integer(20)
        );
        assert_eq!(
            document.get_object((6, 0)).expect("new object"),
            &Object::Integer(60)
        );
    }
}
