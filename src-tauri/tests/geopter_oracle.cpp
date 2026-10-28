// Test adapter around the unmodified upstream Geopter optical library.
#include <algorithm>
#include "system/optical_system.h"
#include "sequential/sequential_trace.h"
#include "analysis/geometrical_mtf.h"
#include "nlohmann/json.hpp"
#include <iostream>
#include <sstream>
using namespace geopter;
int main(int argc,char** argv){
 if(argc<2)return 2;
 const bool geometric = std::string(argv[1])=="--geometric";
 const bool rays = std::string(argv[1])=="--rays";
 const int prescription = (geometric || rays) ? 2 : 1;
 if(argc<=prescription)return 2;
 std::ostringstream chatter;auto output=std::cout.rdbuf(chatter.rdbuf());
 OpticalSystem system;
 // Model-glass examples do not need an external catalog. Named catalog examples do.
 if(argc>prescription+1){std::vector<std::string> files;for(int i=prescription+1;i<argc;i++)files.push_back(argv[i]);system.GetMaterialLib()->LoadAgfFiles(files);}
 system.LoadFile(argv[prescription]);
 if(rays){
  SequentialTrace trace(&system);trace.SetApertureCheck(true);trace.SetApplyVig(false);
  nlohmann::json result=nlohmann::json::array();
  for(int wi=0;wi<system.GetOpticalSpec()->GetWavelengthSpec()->NumberOfWavelengths();wi++){
   double w=system.GetOpticalSpec()->GetWavelengthSpec()->GetWavelength(wi)->Value();
   for(auto p:std::vector<Eigen::Vector2d>{{0.,0.},{1.,0.},{0.5,0.},{0.,1.}}){
    auto ray=trace.CreatePupilRay(p,system.GetOpticalSpec()->GetFieldSpec()->GetField(0),w);
    nlohmann::json points=nlohmann::json::array();
    for(int i=0;i<ray->NumberOfSegments();i++){auto a=ray->GetSegmentAt(i)->IntersectPt();points.push_back({a[0],a[1],a[2]});}
    result.push_back({{"wavelength",w},{"pupil",{p[0],p[1]}},{"status",ray->Status()},{"points",points}});
   }
  }
  std::cout.rdbuf(output);std::cout<<result.dump()<<std::endl;return 0;
 }
 if(geometric){
  GeometricalMTF analyzer;
  const auto plot=analyzer.plot(&system,17,100.1,100.0/64.0);
  nlohmann::json result;result["grid"]=17;result["curves"]=nlohmann::json::array();
  for(int i=0;i<plot->NumberOfGraphs();++i){
   const auto graph=plot->GetGraph(i);
   result["curves"].push_back({{"name",graph->Name()},{"frequencies",graph->XData()},{"values",graph->YData()}});
  }
  std::cout.rdbuf(output);std::cout<<result.dump()<<std::endl;return 0;
 }
 const auto first=system.GetFirstOrderData();
 nlohmann::json result;
 result["efl"]=first->effective_focal_length;result["bfl"]=first->back_focal_length;
 result["entrance_pupil_distance"]=first->entrance_pupil_distance;result["entrance_pupil_radius"]=first->entrance_pupil_radius;
 result["exit_pupil_distance"]=first->exit_pupil_distance;result["exit_pupil_radius"]=first->exit_pupil_radius;
 result["chief_rays"]=nlohmann::json::array();
 SequentialTrace trace(&system);trace.SetApertureCheck(true);trace.SetApplyVig(true);
 double wavelength=system.GetOpticalSpec()->GetWavelengthSpec()->ReferenceWavelength();
 for(int fi=0;fi<system.GetOpticalSpec()->GetFieldSpec()->NumberOfFields();fi++){
  auto field=system.GetOpticalSpec()->GetFieldSpec()->GetField(fi);
  auto ray=trace.CreatePupilRay(Eigen::Vector2d({0.,0.}),field,wavelength);
  if(ray->Status()==TRACE_SUCCESS){auto p=ray->GetBack()->IntersectPt();result["chief_rays"].push_back({p[0],p[1]});}
  else result["chief_rays"].push_back(nullptr);
 }
 std::cout.rdbuf(output);std::cout<<result.dump()<<std::endl;
 return 0;
}
