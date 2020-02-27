// simplifié à 0,005°

const turfBBox = require('@turf/bbox');
const turfIntersect = require('@turf/intersect');
const turfArea = require('@turf/area');

const projgeojson = require('proj-geojson');


const findDep = (geojson, flatbushIndex, depFeatures) => {
    const epsg = geojson.crs.properties.code;
    if (epsg !== 4326){
        geojson = projgeojson(geojson,`EPSG:${epsg}`, 'EPSG:4326',6)
    }

    const feature = geojson.features[0];

    const featureBBOX = turfBBox.default(feature)
    // console.log(featureBBOX)
    // console.log(epsg);
    const found = flatbushIndex.search(featureBBOX[0], featureBBOX[1], featureBBOX[2], featureBBOX[3])
    .map((i) => depFeatures[i]);

    if (found.length == 1){
        return found[0].properties['INSEE_DEP'];
      
    } 
    else {
        // console.log(found);
        const depInterscects = [];
        for (let fou of found){
            const inter = turfIntersect.default(feature,fou );
            if (inter){
                inter.properties['INSEE_DEP'] = fou.properties['INSEE_DEP'];
                depInterscects.push(inter);
            }
        }

        if (depInterscects.length == 1){
            return depInterscects[0].properties['INSEE_DEP'];
        }
        else if ( depInterscects.length > 1){
            let maxArea = 0;
            let selectedFeature = undefined;
            for (let f of depInterscects){
                let area = turfArea.default(f)
                if ( area > maxArea){
                    maxArea = area;
                    selectedFeature = f;
                }
            }
            // turf area prendre le max
            return selectedFeature.properties['INSEE_DEP'];
        }

    }
    

}

module.exports = findDep;