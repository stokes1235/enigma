//! Placement metadata from the client's `lua_rouge_collection_editor.lua`.
//!
//! The Excel export intentionally omits the editor-only shape matrix.  Keeping
//! the compact row masks here lets the server validate the same occupied cells
//! as the client without shipping the full editor table at runtime.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShapeDef {
    pub width: i32,
    pub rows: &'static [u8],
    pub unremovable: bool,
}

pub fn get(item_id: i32) -> Option<ShapeDef> {
    let shape = match item_id {
        8110021 | 8110616 | 8120034 | 8130018 | 8170005 | 8180018 | 8210113 | 8220033 | 8220814
        | 8230040 | 8230050 | 8230414 | 8230509 | 8320506 | 8330002 | 8330417 => ShapeDef {
            width: 2,
            rows: &[3],
            unremovable: false,
        },
        8110029 | 8110030 | 8110213 | 8110312 | 8110716 | 8110805 | 8120035 | 8120216 | 8120317
        | 8120322 | 8120406 | 8120416 | 8120610 | 8120705 | 8120821 | 8130107 | 8130217
        | 8161009 | 8170004 | 8170006 | 8170010 | 8170014 | 8170017 | 8170020 | 8170021
        | 8170025 | 8180004 | 8180006 | 8180009 | 8180013 | 8180016 | 8180019 | 8180021
        | 8180023 | 8210031 | 8210032 | 8210045 | 8210053 | 8210105 | 8210313 | 8210314
        | 8210508 | 8210713 | 8210818 | 8210819 | 8220036 | 8220052 | 8220059 | 8220215
        | 8220322 | 8220409 | 8220415 | 8220515 | 8220516 | 8220806 | 8220813 | 8220903
        | 8221005 | 8230024 | 8230039 | 8261010 | 8310057 | 8310212 | 8310315 | 8310609
        | 8320025 | 8320046 | 8320109 | 8320408 | 8320617 | 8320707 | 8320718 | 8320809
        | 8320812 | 8320822 | 8320905 | 8320906 | 8330001 | 8330003 | 8330005 | 8330063
        | 8330607 | 8330615 | 8330708 | 8330710 | 8330817 | 8330902 | 8331003 | 8350061
        | 8361011 => ShapeDef {
            width: 2,
            rows: &[3, 3],
            unremovable: false,
        },
        8110048 | 8120047 | 8120703 | 8130041 | 8130103 | 8130115 | 8130203 | 8130303 | 8130403
        | 8130404 | 8130503 | 8130603 | 8130605 | 8130803 | 8140101 | 8140201 | 8140301
        | 8140401 | 8140501 | 8140601 | 8140701 | 8140801 | 8161008 | 8170003 | 8170015
        | 8170027 | 8180001 | 8180002 | 8180005 | 8180007 | 8180501 | 8180502 | 8180503
        | 8230004 | 8230019 | 8230027 | 8230028 | 8230037 | 8230042 | 8230304 | 8230305
        | 8230316 | 8230714 | 8231002 | 8240102 | 8240202 | 8240302 | 8240402 | 8240502
        | 8240602 | 8240702 | 8240802 | 8320907 | 8330112 | 8330116 | 8330211 | 8330306
        | 8330311 | 8330412 | 8330413 | 8330513 | 8330816 | 8330904 => ShapeDef {
            width: 1,
            rows: &[1],
            unremovable: false,
        },
        8110307 | 8110321 | 8170026 | 8210205 | 8210308 | 8210321 | 8220106 | 8310405 | 8310608
        | 8330026 => ShapeDef {
            width: 1,
            rows: &[1, 1, 1],
            unremovable: false,
        },
        8110410 | 8120104 | 8120208 | 8120704 | 8130315 | 8130504 | 8130505 | 8130604 | 8130804
        | 8131001 | 8150006 | 8150008 | 8150010 | 8150012 | 8150014 | 8150016 | 8170002
        | 8170007 | 8170011 | 8170012 | 8170013 | 8170016 | 8170023 | 8170030 | 8180003
        | 8180008 | 8180015 | 8180017 | 8210056 | 8210058 | 8220044 | 8220051 | 8220318
        | 8230204 | 8230606 | 8230901 | 8250007 | 8250009 | 8250011 | 8250013 | 8250015
        | 8250017 | 8250055 | 8261006 | 8320049 | 8320320 | 8320514 | 8320618 | 8330020
        | 8330023 | 8330111 | 8330218 | 8330715 | 8350043 | 8350060 | 8350717 | 8361007 => {
            ShapeDef {
                width: 1,
                rows: &[1, 1],
                unremovable: false,
            }
        }
        8110507 | 8170001 | 8180010 | 8180011 => ShapeDef {
            width: 3,
            rows: &[7],
            unremovable: false,
        },
        8120110 | 8170018 | 8180022 | 8220612 | 8220807 | 8220820 | 8310706 | 8310810 | 8320062
        | 8320207 | 8320511 => ShapeDef {
            width: 2,
            rows: &[3, 3, 3],
            unremovable: false,
        },
        8120512 => ShapeDef {
            width: 2,
            rows: &[1, 3],
            unremovable: false,
        },
        8120808 | 8170008 | 8170009 | 8170022 | 8180020 | 8180024 | 8210711 | 8210712 | 8220214
        | 8220613 | 8310114 | 8310419 | 8320210 | 8320310 | 8320323 | 8330219 | 8330418
        | 8360054 => ShapeDef {
            width: 3,
            rows: &[7, 7],
            unremovable: false,
        },
        8170019 => ShapeDef {
            width: 2,
            rows: &[3, 2, 2],
            unremovable: false,
        },
        8170024 => ShapeDef {
            width: 2,
            rows: &[3, 3, 2],
            unremovable: false,
        },
        8170028 => ShapeDef {
            width: 2,
            rows: &[1, 3, 3],
            unremovable: false,
        },
        8170029 => ShapeDef {
            width: 3,
            rows: &[2, 7, 7],
            unremovable: false,
        },
        8180012 | 8311004 => ShapeDef {
            width: 3,
            rows: &[4, 7],
            unremovable: false,
        },
        8180014 => ShapeDef {
            width: 4,
            rows: &[15],
            unremovable: false,
        },
        8210209 => ShapeDef {
            width: 3,
            rows: &[7, 2],
            unremovable: false,
        },
        8210411 | 8310108 | 8310206 => ShapeDef {
            width: 1,
            rows: &[1, 1, 1, 1],
            unremovable: false,
        },
        8210611 | 8310510 => ShapeDef {
            width: 3,
            rows: &[2, 7, 2],
            unremovable: false,
        },
        8220619 => ShapeDef {
            width: 2,
            rows: &[3, 3],
            unremovable: true,
        },
        8230038 | 8230614 => ShapeDef {
            width: 2,
            rows: &[2, 3],
            unremovable: false,
        },
        8310022 => ShapeDef {
            width: 4,
            rows: &[1, 1, 15],
            unremovable: false,
        },
        8310309 => ShapeDef {
            width: 3,
            rows: &[7, 2, 2],
            unremovable: false,
        },
        8310407 => ShapeDef {
            width: 2,
            rows: &[3, 3, 1, 1],
            unremovable: false,
        },
        8310517 => ShapeDef {
            width: 3,
            rows: &[4, 6, 3],
            unremovable: false,
        },
        8310815 => ShapeDef {
            width: 3,
            rows: &[6, 3],
            unremovable: false,
        },
        8320709 => ShapeDef {
            width: 3,
            rows: &[1, 1, 7],
            unremovable: false,
        },
        8320811 => ShapeDef {
            width: 3,
            rows: &[7, 7, 7],
            unremovable: false,
        },
        8330620 => ShapeDef {
            width: 1,
            rows: &[1],
            unremovable: true,
        },
        _ => return None,
    };
    Some(shape)
}

/// Number of enchant holes declared by the same editor table.
pub fn hole_num(item_id: i32) -> i32 {
    match item_id {
        8110307 | 8110410 | 8110507 | 8110616 | 8110805 | 8120406 | 8120512 | 8120610 | 8120705
        | 8170002 | 8170003 | 8170005 | 8170007 | 8170011 | 8170012 | 8170013 | 8170015
        | 8170016 | 8170023 | 8170027 | 8170030 | 8180002 | 8180003 | 8180005 | 8180007
        | 8180011 | 8180012 | 8180013 | 8180015 | 8180016 | 8180023 | 8210031 | 8210105
        | 8210113 | 8210205 | 8210209 | 8210508 | 8210818 | 8220106 | 8220415 | 8310405
        | 8310419 | 8310517 | 8330620 | 8350717 => 1,
        8110312 | 8120110 | 8120208 | 8120808 | 8170001 | 8170004 | 8170006 | 8170010 | 8170014
        | 8170017 | 8170019 | 8170020 | 8170021 | 8170024 | 8170025 | 8170026 | 8180001
        | 8180004 | 8180006 | 8180010 | 8180014 | 8180017 | 8180019 | 8180020 | 8180021
        | 8180024 | 8210313 | 8210411 | 8210611 | 8210819 | 8220409 | 8220613 | 8220807
        | 8220813 | 8220814 | 8310206 | 8310609 | 8310815 | 8320323 | 8320709 | 8320718
        | 8320809 | 8320812 | 8320822 | 8330116 | 8330219 => 2,
        8170008 | 8170009 | 8170018 | 8170022 | 8170028 | 8170029 | 8180008 | 8180009 | 8180018
        | 8180022 | 8310022 | 8310114 | 8310407 | 8310510 | 8310706 | 8310810 => 3,
        _ => 0,
    }
}

pub fn is_enchant_item(item_id: i32) -> bool {
    matches!(
        item_id,
        8140101
            | 8140201
            | 8140301
            | 8140401
            | 8140501
            | 8140601
            | 8140701
            | 8140801
            | 8240102
            | 8240202
            | 8240302
            | 8240402
            | 8240502
            | 8240602
            | 8240702
            | 8240802
    )
}

/// Editor-only `isUnique` flag from `lua_rouge_collection_editor.lua`.
pub fn is_unique(item_id: i32) -> bool {
    matches!(
        item_id,
        8170001..=8170030
            | 8180001..=8180024
            | 8210209
            | 8310206
            | 8310405
            | 8310608
            | 8320207
            | 8320310
            | 8320617
            | 8330418
    )
}

/// Occupied cells relative to the top-left placement position. Rotation values
/// follow the client's 0/90/180/270 clockwise enum (0..=3).
pub fn cells(item_id: i32, rotation: i32) -> Option<Vec<(i32, i32)>> {
    let shape = get(item_id)?;
    if !(0..=3).contains(&rotation) {
        return None;
    }

    let height = i32::try_from(shape.rows.len()).ok()?;
    let mut result = Vec::new();
    for (row, mask) in shape.rows.iter().copied().enumerate() {
        let row = i32::try_from(row).ok()?;
        for col in 0..shape.width {
            if mask & (1 << col) == 0 {
                continue;
            }
            let cell = match rotation {
                0 => (row, col),
                1 => (col, height - row - 1),
                2 => (height - row - 1, shape.width - col - 1),
                3 => (shape.width - col - 1, row),
                _ => unreachable!(),
            };
            result.push(cell);
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_the_same_way_as_the_client_matrix() {
        assert_eq!(cells(8120512, 0).unwrap(), vec![(0, 0), (1, 0), (1, 1)]);
        assert_eq!(cells(8120512, 1).unwrap(), vec![(0, 1), (0, 0), (1, 0)]);
        assert_eq!(cells(8120512, 2).unwrap(), vec![(1, 1), (0, 1), (0, 0)]);
        assert_eq!(cells(8120512, 3).unwrap(), vec![(1, 0), (1, 1), (0, 1)]);
    }

    #[test]
    fn enchant_items_follow_the_client_type_105_rows() {
        assert!(get(8140101).is_some());
        assert!(is_enchant_item(8140101));
        assert!(is_enchant_item(8240802));
        assert!(!is_enchant_item(8180201));
        assert!(!is_enchant_item(8230024));
    }

    #[test]
    fn reports_editor_defined_enchant_holes() {
        assert_eq!(hole_num(8230024), 0);
        assert_eq!(hole_num(8310405), 1);
        assert_eq!(hole_num(8310206), 2);
        assert_eq!(hole_num(8310114), 3);
    }
}
