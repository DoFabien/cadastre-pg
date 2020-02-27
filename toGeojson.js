const decompress = require("decompress");
const edigeoTogeojson = require("edigeo-to-geojson");

const fromText = _path => {
    const pathBz = path.join(__dirname, _path);
    const filesNames = fs.readdirSync(pathBz);
  
    const bufferData = {
      THF: undefined,
      QAL: undefined,
      GEO: undefined,
      SCD: undefined,
      VEC: []
    };
    for (let i = 0; i < filesNames.length; i++) {
      if (/\.THF$/.test(filesNames[i])) {
        bufferData.THF = fs.readFileSync(path.join(pathBz, filesNames[i]));
      } else if (/\.VEC$/.test(filesNames[i])) {
        bufferData.VEC.push(fs.readFileSync(path.join(pathBz, filesNames[i])));
      } else if (/\.QAL$/.test(filesNames[i])) {
        bufferData.QAL = fs.readFileSync(path.join(pathBz, filesNames[i]));
      } else if (/\.GEO$/.test(filesNames[i])) {
        bufferData.GEO = fs.readFileSync(path.join(pathBz, filesNames[i]));
      } else if (/\.SCD$/.test(filesNames[i])) {
        bufferData.SCD = fs.readFileSync(path.join(pathBz, filesNames[i]));
      }
    }
    return edigeoTogeojson(bufferData);
  };
  
  const fromCompressed = async _path => {
    const files = await decompress(_path);
    const bufferData = {
      THF: undefined,
      QAL: undefined,
      GEO: undefined,
      SCD: undefined,
      VEC: []
    };
    for (let i = 0; i < files.length; i++) {
      if (/\.THF$/.test(files[i].path)) {
        bufferData.THF = files[i].data;
      } else if (/\.VEC$/.test(files[i].path)) {
        bufferData.VEC.push(files[i].data);
      } else if (/\.QAL$/.test(files[i].path)) {
        bufferData.QAL = files[i].data;
      } else if (/\.GEO$/.test(files[i].path)) {
        bufferData.GEO = files[i].data;
      } else if (/\.SCD$/.test(files[i].path)) {
        bufferData.SCD = files[i].data;
      }
    }
  
    return edigeoTogeojson(bufferData);
  };


  module.exports = { fromText, fromCompressed }