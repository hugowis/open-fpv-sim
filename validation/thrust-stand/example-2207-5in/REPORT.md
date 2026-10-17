# Thrust-stand comparison: Illustrative 2207 1950 KV motor, 5-inch propeller, 6S (synthetic)

> **These figures are estimates, not measurements.** The comparison shows the format and the report, not how accurate the model is.

- Source: `estimated` — Hand-written, datasheet-style figures that show the format and the report. NOT a measurement of any motor.
- Contributor: Open FPV Sim maintainers (CC-BY-SA-4.0)
- Quad: `../../../quads/opendrone-5f-freestyle.toml`, motor 0
- Rows: 6

Errors are (simulated − measured) / measured, in percent. A dash is a figure the stand did not record.

| command | voltage (V) | rpm meas | rpm sim | err % | thrust meas (N) | thrust sim | err % | current meas (A) | current sim | err % |
|---|---|---|---|---|---|---|---|---|---|---|
| 0.20 | 24.0 | 14000 | 9014 | -35.6 | 2.20 | 0.79 | -64.0 | 3.0 | 0.5 | -83.1 |
| 0.30 | 23.9 | 18000 | 13414 | -25.5 | 3.60 | 1.75 | -51.3 | — | 1.2 | — |
| 0.40 | 23.8 | 21500 | 17697 | -17.7 | 5.10 | 3.05 | -40.2 | 11.0 | 2.5 | -76.9 |
| 0.60 | 23.4 | 27000 | 25729 | -4.7 | 8.00 | 6.45 | -19.4 | 24.0 | 7.3 | -69.8 |
| 0.80 | 22.9 | 31000 | 33101 | +6.8 | 10.60 | 10.67 | +0.7 | 38.0 | 15.4 | -59.5 |
| 1.00 | 22.4 | 34000 | 39937 | +17.5 | 12.70 | 15.53 | +22.3 | 52.0 | 27.4 | -47.3 |

| quantity | rows compared | mean absolute error % | maximum absolute error % |
|---|---|---|---|
| rpm | 6 | 18.0 | 35.6 |
| thrust | 6 | 33.0 | 64.0 |
| battery current | 5 | 67.3 | 83.1 |
