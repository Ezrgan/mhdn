//! In-game species id to the English name in the MHGU monster table.
//!
//! Spellings are the table's Name column, not a translation:
//! <https://github.com/RTHKKona/MHGU-Modding/wiki/Monster-IDs>
//! The corner atlas has no lowercase, so the overlay draws these in capitals.

/// `(in-game id, name)`. 94 rows, the whole table.
pub const SPECIES_NAMES: &[(u16, &str)] = &[
    (1, "Rathian"),
    (2, "Rathalos"),
    (3, "Khezu"),
    (4, "Basarios"),
    (5, "Gravios"),
    (7, "Diablos"),
    (8, "Yian Kut-Ku"),
    (9, "Gypceros"),
    (10, "Plesioth"),
    (11, "Kirin"),
    (12, "LaoShanLung"),
    (13, "Fatalis"),
    (14, "Velocidrome"),
    (15, "Gendrome"),
    (16, "Iodrome"),
    (17, "Cephadrome"),
    (18, "Yian Garuga"),
    (19, "Daimyo Hermitaur"),
    (20, "Shogun Ceanataur"),
    (21, "Congalala"),
    (22, "Blangonga"),
    (23, "Rajang"),
    (24, "Kushala Daora"),
    (25, "Chameleos"),
    (27, "Teostra"),
    (30, "Bulldrome"),
    (32, "Tigrex"),
    (33, "Akantor"),
    (34, "Giadrome"),
    (36, "Lavasioth"),
    (37, "Nargacuga"),
    (38, "Ukanlos"),
    (42, "Barioth"),
    (43, "Deviljho"),
    (44, "Barroth"),
    (45, "Uragaan"),
    (46, "Lagiacrus"),
    (47, "Royal Ludroth"),
    (49, "Agnaktor"),
    (50, "Alatreon"),
    (55, "Duramboros"),
    (56, "Nibelsnarf"),
    (57, "Zinogre"),
    (58, "Amatsu"),
    (60, "Arzuros"),
    (61, "Lagombi"),
    (62, "Volvidon"),
    (63, "Brachydios"),
    (65, "Kecha Wacha"),
    (66, "Tetsucabra"),
    (67, "Zamtrios"),
    (68, "Najarala"),
    (69, "Seltas Queen"),
    (70, "Nerscylla"),
    (71, "Gore Magala"),
    (72, "Shagaru Magala"),
    (76, "Seltas"),
    (77, "Seregios"),
    (79, "Malfestio"),
    (80, "Glavenus"),
    (81, "Astalos"),
    (82, "Mizutsune"),
    (83, "Gammoth"),
    (84, "Nakarkos"),
    (85, "Great Maccao"),
    (86, "Valstrax"),
    (87, "Ahtal-Neset"),
    (88, "Ahtal-Ka"),
    (269, "Crimson Fatalis"),
    (513, "GoldRathian"),
    (514, "Silver Rathalos"),
    (525, "White Fatalis"),
    (1025, "Dreadqueen Rathian"),
    (1026, "Dreadking Rathalos"),
    (1031, "Bloodbath Diablos"),
    (1042, "Dead-Eye Yian Garuga"),
    (1043, "Stonefist Hermitaur"),
    (1044, "Rustrazor Ceanataur"),
    (1056, "Grimclaw Tigrex"),
    (1061, "Silverwind Nargacuga"),
    (1069, "Crystalbeard Uragaan"),
    (1081, "Thunderlord Zinogre"),
    (1084, "Redhelm Arzuros"),
    (1085, "Snowbaron Lagombi"),
    (1090, "Drilltusk Tetsucabra"),
    (1103, "Nightcloak Malfestio"),
    (1104, "Hellblade Glavenus"),
    (1105, "Boltreaver Astalos"),
    (1106, "Soulseer Mizutsune"),
    (1107, "Elderfrost Gammoth"),
    (1303, "Furious Rajang"),
    (1323, "Savage Deviljho"),
    (1343, "Raging Brachydios"),
    (1351, "Chaotic Gore Magala"),
];

/// Table spelling for `species`, or `None` when that id is not in the table.
pub fn name(species: u16) -> Option<&'static str> {
    SPECIES_NAMES
        .iter()
        .find(|&&(id, _)| id == species)
        .map(|&(_, monster)| monster)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_measured_ids_use_the_table_names() {
        assert_eq!(SPECIES_NAMES.len(), 94);
        assert_eq!(name(1), Some("Rathian"));
        assert_eq!(name(14), Some("Velocidrome"));
        assert_eq!(name(30), Some("Bulldrome"));
        assert_eq!(name(85), Some("Great Maccao"));
        assert!(name(116).is_none());
    }

    #[test]
    fn every_id_is_unique_and_the_corner_can_draw_the_name() {
        let mut seen = std::collections::BTreeSet::new();
        for &(id, monster) in SPECIES_NAMES {
            assert!(seen.insert(id), "duplicate {id}");
            let corner = monster.to_ascii_uppercase();
            assert!(
                corner.chars().all(|ch| matches!(ch, ' ' | 'A'..='Z' | '-')),
                "{monster} needs a glyph the corner atlas does not have"
            );
        }
    }
}
