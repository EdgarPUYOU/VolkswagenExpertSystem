-- Numeric business facts moved out of the TTL files. SQLite is the source of truth; the app writes them
-- into the knowledge graph (graph/erp-facts) at load time so the rules still see them.
-- Projected component volume is no longer stored: it is the sum of planned production of the models using the component.

CREATE TABLE model_facts (
    model_iri          TEXT NOT NULL PRIMARY KEY,
    price_eur          REAL NOT NULL,
    planned_units      REAL NOT NULL,
    produced_last_year REAL NOT NULL
);

CREATE TABLE contract_terms (
    contract_iri          TEXT NOT NULL PRIMARY KEY,
    unit_price_eur        REAL NOT NULL,
    committed_volume      REAL NOT NULL,
    discount_price_eur    REAL,
    discount_tier_volume  REAL
);

INSERT INTO model_facts VALUES ('http://example.org/volkswagen/911', 125000.0, 55000.0, 50000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/A4', 45000.0, 80000.0, 70000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Cayenne', 90000.0, 105000.0, 90000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Enyaq', 42000.0, 125000.0, 90000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Golf', 30000.0, 300000.0, 250000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Huracan', 250000.0, 4000.0, 3000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/ID4', 45000.0, 140000.0, 100000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Octavia', 32000.0, 180000.0, 150000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Passat', 40000.0, 120000.0, 100000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Q5', 55000.0, 150000.0, 120000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/RSeries', 150000.0, 100000.0, 80000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/TGX', 140000.0, 75000.0, 60000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Taycan', 100000.0, 45000.0, 30000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/Urus', 230000.0, 8000.0, 6000.0);
INSERT INTO model_facts VALUES ('http://example.org/volkswagen/e-tron', 70000.0, 55000.0, 40000.0);

INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_01_EA211_15TSI', 2200.0, 525000.0, 2050.0, 580000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_02_ID4ElectricDrive', 4500.0, 200000.0, 4200.0, 250000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_03_Porsche911Engine', 9000.0, 180000.0, 8700.0, 190000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_04_A4TSI', 3600.0, 200000.0, 3400.0, 220000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_05_ETronElectricDrive', 5000.0, 90000.0, 4800.0, 115000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_06_ScaniaRSeriesEngine', 15000.0, 160000.0, 14500.0, 180000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_07_StandardPassengerSeat', 450.0, 960000.0, 420.0, 1100000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_08_PerformanceSeat', 1250.0, 225000.0, 1200.0, 240000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_09_CommercialSeat', 700.0, 150000.0, 650.0, 170000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_10_BatteryPack_MEB', 7800.0, 200000.0, 7400.0, 250000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_11_BatteryPack_J1', 11000.0, 42000.0, 10500.0, 50000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_12_BatteryPack_Etron', 8500.0, 50000.0, 8200.0, 60000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_13_Transmission_Standard', 1400.0, 960000.0, 1320.0, 1100000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_14_Transmission_Performance', 2600.0, 225000.0, 2450.0, 240000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_15_Transmission_HeavyDuty', 3500.0, 150000.0, 3300.0, 170000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_16_BrakeSystem_Standard', 700.0, 1160000.0, 650.0, 1200000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_17_BrakeSystem_Performance', 1600.0, 225000.0, 1500.0, 240000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_18_BrakeSystem_HeavyDuty', 2200.0, 180000.0, 2050.0, 190000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_19_Infotainment_Standard', 900.0, 1160000.0, 850.0, 1150000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_20_Infotainment_Premium', 1800.0, 225000.0, 1700.0, 240000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_21_Infotainment_Commercial', 1200.0, 150000.0, 1120.0, 170000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_22_TyreSet_Standard', 600.0, 1140000.0, 560.0, 1200000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_23_TyreSet_Performance', 1200.0, 220000.0, 1100.0, 240000.0);
INSERT INTO contract_terms VALUES ('http://example.org/volkswagen/SupplyContract_24_TyreSet_HeavyDuty', 1800.0, 160000.0, 1650.0, 180000.0);
