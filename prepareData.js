const crypto = require("crypto")
const projgeojson = require('proj-geojson');


const functions = ( DEP, MILLESIME) => {

   return {
    'addMillesime': (val) => MILLESIME,
    'addDep': (val) => `${DEP}${val}`,
    'toInt': (val) => {
        const int = parseInt(val)
        return  int ? int : null;
    },
    'toFloat': (val) => {
        if (!val) return null;
        const numberPattern = /\d+\.?\d+?/g;
        const n = val.match(numberPattern)
        if (!n) return null;
        return parseFloat(n[0])
    },
    'toDate': (val) => {
        if (!val) return null;
        const DD = val.substring(6, 8);
        const MM = val.substring(4, 6);
        const YYYY = val.substring(0, 4);
        let date = new Date(`${YYYY}-${MM}-${DD}`)
        if (isNaN(date) || YYYY < 1000) {
            return null
        }
        return date
    },
    'toDateFR': (val) => {
        if (!val) return null;
        val = val.replace(/\//g, '');
        const DD = val.substring(0, 2);
        const MM = val.substring(2, 4);
        const YYYY = val.substring(4, 8);
        let date = new Date(`${YYYY}-${MM}-${DD}`)
        if (isNaN(date) || YYYY < 1000) {
            return null
        }
        return date
    }
   }
   
}



const prepareData = function (edigeoConfig, data, consts,DEP, MILLESIME, ESPSGcode) {
    const geojsons = {};

    for (const idType in edigeoConfig.tableConfig) {
        const currentConf = edigeoConfig.tableConfig[idType];

            const geojsonData = data.geojsons[idType]
            if (!geojsonData) continue;
      
            const confFields = currentConf.fields
            geojsons[currentConf.table] = {
                "type": "FeatureCollection",
                "features": []
            }

            for (feature of geojsonData.features) {
                const preparedFeature = { "type": "Feature", "geometry": feature.geometry, "properties": {} }

                if (currentConf.hashGeom) {
                    const geomhash = crypto.createHmac('sha256', JSON.stringify(feature.geometry)).digest('hex');
                    preparedFeature.properties['geomhash'] = geomhash
                }

                for (let confField of confFields) {

                    let val = feature.properties[confField.json];

                    if (confField.const){
                        val = consts[confField.const];
                    }

                    if (confField.functions && confField.functions.length > 0) {
                        val = confField.functions.reduce((_val, f) => functions(DEP, MILLESIME)[f](_val), val);
                    }
                    preparedFeature.properties[confField.db] = val;
                }
                geojsons[currentConf.table].features.push(preparedFeature);
            }
            //reprojection  
        
            const inputEPSGcode = data.geojsons[idType].crs.properties.code;
            if (inputEPSGcode !== ESPSGcode){
                geojsons[currentConf.table] = projgeojson(geojsons[currentConf.table], `EPSG:${inputEPSGcode}`, `EPSG:${ESPSGcode}`, 7);
            }


         
    }
    return ({ 'geojsons': geojsons})
}

module.exports = prepareData;